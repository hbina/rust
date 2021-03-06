use rustc_infer::infer::nll_relate::{NormalizationStrategy, TypeRelating, TypeRelatingDelegate};
use rustc_infer::infer::{InferCtxt, NllRegionVariableOrigin};
use rustc_middle::mir::ConstraintCategory;
use rustc_middle::ty::relate::TypeRelation;
use rustc_middle::ty::{self, Const, Ty};
use rustc_trait_selection::traits::query::Fallible;

use crate::borrow_check::constraints::OutlivesConstraint;
use crate::borrow_check::type_check::{BorrowCheckContext, Locations};

/// Adds sufficient constraints to ensure that `a R b` where `R` depends on `v`:
///
/// - "Covariant" `a <: b`
/// - "Invariant" `a == b`
/// - "Contravariant" `a :> b`
///
/// N.B., the type `a` is permitted to have unresolved inference
/// variables, but not the type `b`.
pub(super) fn relate_types<'tcx>(
    infcx: &InferCtxt<'_, 'tcx>,
    a: Ty<'tcx>,
    v: ty::Variance,
    b: Ty<'tcx>,
    locations: Locations,
    category: ConstraintCategory,
    borrowck_context: Option<&mut BorrowCheckContext<'_, 'tcx>>,
) -> Fallible<()> {
    debug!("relate_types(a={:?}, v={:?}, b={:?}, locations={:?})", a, v, b, locations);
    TypeRelating::new(
        infcx,
        NllTypeRelatingDelegate::new(infcx, borrowck_context, locations, category),
        v,
    )
    .relate(a, b)?;
    Ok(())
}

struct NllTypeRelatingDelegate<'me, 'bccx, 'tcx> {
    infcx: &'me InferCtxt<'me, 'tcx>,
    borrowck_context: Option<&'me mut BorrowCheckContext<'bccx, 'tcx>>,

    /// Where (and why) is this relation taking place?
    locations: Locations,

    /// What category do we assign the resulting `'a: 'b` relationships?
    category: ConstraintCategory,
}

impl NllTypeRelatingDelegate<'me, 'bccx, 'tcx> {
    fn new(
        infcx: &'me InferCtxt<'me, 'tcx>,
        borrowck_context: Option<&'me mut BorrowCheckContext<'bccx, 'tcx>>,
        locations: Locations,
        category: ConstraintCategory,
    ) -> Self {
        Self { infcx, borrowck_context, locations, category }
    }
}

impl TypeRelatingDelegate<'tcx> for NllTypeRelatingDelegate<'_, '_, 'tcx> {
    fn create_next_universe(&mut self) -> ty::UniverseIndex {
        self.infcx.create_next_universe()
    }

    fn next_existential_region_var(&mut self, from_forall: bool) -> ty::Region<'tcx> {
        if self.borrowck_context.is_some() {
            let origin = NllRegionVariableOrigin::Existential { from_forall };
            self.infcx.next_nll_region_var(origin)
        } else {
            self.infcx.tcx.lifetimes.re_erased
        }
    }

    fn next_placeholder_region(&mut self, placeholder: ty::PlaceholderRegion) -> ty::Region<'tcx> {
        if let Some(borrowck_context) = &mut self.borrowck_context {
            borrowck_context.constraints.placeholder_region(self.infcx, placeholder)
        } else {
            self.infcx.tcx.lifetimes.re_erased
        }
    }

    fn generalize_existential(&mut self, universe: ty::UniverseIndex) -> ty::Region<'tcx> {
        self.infcx.next_nll_region_var_in_universe(
            NllRegionVariableOrigin::Existential { from_forall: false },
            universe,
        )
    }

    fn push_outlives(&mut self, sup: ty::Region<'tcx>, sub: ty::Region<'tcx>) {
        if let Some(borrowck_context) = &mut self.borrowck_context {
            let sub = borrowck_context.universal_regions.to_region_vid(sub);
            let sup = borrowck_context.universal_regions.to_region_vid(sup);
            borrowck_context.constraints.outlives_constraints.push(OutlivesConstraint {
                sup,
                sub,
                locations: self.locations,
                category: self.category,
            });
        }
    }

    // We don't have to worry about the equality of consts during borrow checking
    // as consts always have a static lifetime.
    fn const_equate(&mut self, _a: &'tcx Const<'tcx>, _b: &'tcx Const<'tcx>) {}

    fn normalization() -> NormalizationStrategy {
        NormalizationStrategy::Eager
    }

    fn forbid_inference_vars() -> bool {
        true
    }
}
