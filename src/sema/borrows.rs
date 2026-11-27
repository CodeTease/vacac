use super::*;

pub(super) enum ReferencePlaceAccess {
    Raw {
        target_type: ValueType,
    },
    Safe {
        target_type: ValueType,
        provenance: ReferenceProvenance,
        projections: Vec<BorrowProjection>,
        kind: BorrowKind,
    },
}

impl Checker {
    pub(super) fn push_loan_scope(&mut self) {
        self.loans.push(Vec::new());
    }

    pub(super) fn pop_loan_scope(&mut self) {
        self.loans.pop();
    }

    pub(super) fn next_loan(&mut self, place: BorrowPlace, kind: BorrowKind) -> usize {
        let id = self.next_loan_id;
        self.next_loan_id += 1;
        self.loans
            .last_mut()
            .expect("loan scope exists")
            .push(ActiveLoan { id, place, kind });
        id
    }

    pub(super) fn parameter_loan(
        &mut self,
        index: usize,
        lifetime: Option<String>,
        kind: BorrowKind,
    ) -> ReferenceProvenance {
        let callable = self.current_callable_id;
        let id = self.next_loan(BorrowPlace::parameter(callable, index), kind);
        ReferenceProvenance::parameter(index, lifetime, id, callable)
    }

    /// Retain a new reference value. Every possible origin gains one child loan,
    /// which is what keeps the parent unavailable until this binding's scope ends.
    pub(super) fn bind_reference(
        &mut self,
        mut provenance: ReferenceProvenance,
        kind: BorrowKind,
        span: Span,
    ) -> Result<ReferenceProvenance, Diagnostic> {
        if provenance.is_unknown() && kind == BorrowKind::Mutable {
            return Err(at(
                span,
                "cannot establish the origin of this mutable reference",
            ));
        }
        let loan_id = self.next_loan_id;
        self.next_loan_id += 1;
        for source in &mut provenance.sources {
            let Some(place) = source.place().cloned() else {
                continue;
            };
            let Some(loans) = source.loans_mut() else {
                continue;
            };
            loans.push(loan_id);
            self.loans
                .last_mut()
                .expect("loan scope exists")
                .push(ActiveLoan {
                    id: loan_id,
                    place,
                    kind,
                });
            source.as_derived();
        }
        Ok(provenance)
    }

    /// Reserve references that are only live for the duration of a call.
    pub(super) fn reserve_reference_argument(
        &mut self,
        provenance: &ReferenceProvenance,
        kind: BorrowKind,
    ) {
        let mut places = Vec::new();
        for source in &provenance.sources {
            if let Some(place) = source.place()
                && !places.contains(place)
            {
                places.push(place.clone());
            }
        }
        for place in places {
            self.next_loan(place, kind);
        }
    }

    fn reference_access_conflict(
        &self,
        place: &BorrowPlace,
        loans: &[usize],
        mode: AccessMode,
    ) -> Option<Diagnostic> {
        for loan in self.loans.iter().flatten() {
            if loans.contains(&loan.id) || !loan.place.overlaps(place) {
                continue;
            }
            if !loan.kind.conflicts_with(mode) {
                continue;
            }
            let name = place.root_name();
            let message = match (mode, loan.kind) {
                (AccessMode::Read, BorrowKind::Mutable) => {
                    format!("cannot access `{name}` while it is mutably borrowed")
                }
                (AccessMode::Write, BorrowKind::Mutable) => {
                    format!("cannot access `{name}` while it is mutably borrowed")
                }
                (AccessMode::Write, BorrowKind::Shared) => {
                    format!("cannot modify `{name}` while it is borrowed")
                }
                (AccessMode::BorrowMutable, BorrowKind::Mutable) => {
                    format!("`{name}` is already mutably borrowed")
                }
                (AccessMode::BorrowMutable, BorrowKind::Shared) => {
                    format!("`{name}` is already borrowed")
                }
                (AccessMode::BorrowShared, BorrowKind::Mutable) => format!(
                    "cannot immutably borrow `{name}` because it is already mutably borrowed"
                ),
                (AccessMode::Read, BorrowKind::Shared)
                | (AccessMode::BorrowShared, BorrowKind::Shared) => unreachable!(),
            };
            return Some(Diagnostic::new(message));
        }
        None
    }

    pub(super) fn check_reference_access(
        &self,
        provenance: &ReferenceProvenance,
        projections: &[BorrowProjection],
        mode: AccessMode,
        span: Span,
    ) -> Result<(), Diagnostic> {
        for source in &provenance.sources {
            let Some(place) = source.place() else {
                return Err(at(
                    span,
                    "cannot access a reference whose origin cannot be established",
                ));
            };
            let mut place = place.clone();
            place.projections.extend_from_slice(projections);
            if let Some(error) = self.reference_access_conflict(&place, source.loans(), mode) {
                return Err(at(span, error.message));
            }
        }
        Ok(())
    }

    /// Return the reference expression at the base of a place path and the
    /// projections applied to its pointee, e.g. `(*r).left[0]`.
    ///
    /// `check_selectors` is for expression typing. Provenance lookups run after
    /// (or while) the surrounding expression is checked, so they must not
    /// re-check index expressions just to recover the same projections.
    pub(super) fn reborrow_parts<'a>(
        &mut self,
        expr: &'a Expr,
        check_selectors: bool,
    ) -> Result<Option<(&'a Expr, Vec<BorrowProjection>)>, Diagnostic> {
        fn collect<'a>(
            checker: &mut Checker,
            expr: &'a Expr,
            projections: &mut Vec<BorrowProjection>,
            check_selectors: bool,
        ) -> Result<Option<&'a Expr>, Diagnostic> {
            match expr {
                Expr::Unary {
                    op: UnaryOp::Dereference,
                    value,
                    ..
                } => Ok(Some(value)),
                Expr::Field { value, name, .. } => {
                    let Some(reference) = collect(checker, value, projections, check_selectors)?
                    else {
                        return Ok(None);
                    };
                    projections.push(BorrowProjection::Field(name.clone()));
                    Ok(Some(reference))
                }
                Expr::Index { array, index, .. } => {
                    let Some(reference) = collect(checker, array, projections, check_selectors)?
                    else {
                        return Ok(None);
                    };
                    if check_selectors {
                        let index_type = checker.check_expr(index, false)?;
                        checker.expect_type(
                            index_type,
                            ValueType::I64,
                            span_of_expr(index),
                            "array index",
                        )?;
                    }
                    projections.push(BorrowProjection::Index(constant_i64(index)));
                    Ok(Some(reference))
                }
                _ => Ok(None),
            }
        }

        let mut projections = Vec::new();
        let reference = collect(self, expr, &mut projections, check_selectors)?;
        Ok(reference.map(|reference| (reference, projections)))
    }

    pub(super) fn reference_place_access(
        &mut self,
        expr: &Expr,
        span: Span,
    ) -> Result<Option<ReferencePlaceAccess>, Diagnostic> {
        let Some((reference, projections)) = self.reborrow_parts(expr, true)? else {
            return Ok(None);
        };
        let reference_type = self.check_expr(reference, false)?;
        let (target, provenance, kind) = match reference_type {
            ValueType::Reference(index) => {
                let target = self.reference_types[index];
                let provenance = self
                    .reference_origin(reference)?
                    .ok_or_else(|| at(span, "cannot establish the origin of this reference"))?;
                (target, provenance, BorrowKind::Shared)
            }
            ValueType::MutableReference(index) => {
                let target = self.mutable_reference_types[index];
                let provenance = self
                    .reference_origin(reference)?
                    .ok_or_else(|| at(span, "cannot establish the origin of this reference"))?;
                (target, provenance, BorrowKind::Mutable)
            }
            ValueType::RawPointer(index) => {
                let target = self.raw_pointer_types[index].0;
                let target_type = self.project_place_type(target, &projections, span)?;
                return Ok(Some(ReferencePlaceAccess::Raw { target_type }));
            }
            other => {
                return Err(at(
                    span,
                    format!("cannot dereference {}", self.type_name(other)),
                ));
            }
        };
        let target_type = self.project_place_type(target, &projections, span)?;
        Ok(Some(ReferencePlaceAccess::Safe {
            target_type,
            provenance,
            projections,
            kind,
        }))
    }

    pub(super) fn reference_access_type(
        &mut self,
        expr: &Expr,
        span: Span,
    ) -> Result<
        Option<(
            ValueType,
            ReferenceProvenance,
            Vec<BorrowProjection>,
            BorrowKind,
        )>,
        Diagnostic,
    > {
        match self.reference_place_access(expr, span)? {
            Some(ReferencePlaceAccess::Safe {
                target_type,
                provenance,
                projections,
                kind,
            }) => Ok(Some((target_type, provenance, projections, kind))),
            Some(ReferencePlaceAccess::Raw { .. }) => Err(at(
                span,
                "safe references cannot be formed through raw pointers",
            )),
            None => Ok(None),
        }
    }

    pub(super) fn project_place_type(
        &mut self,
        mut ty: ValueType,
        projections: &[BorrowProjection],
        span: Span,
    ) -> Result<ValueType, Diagnostic> {
        for projection in projections {
            match projection {
                BorrowProjection::Field(name) => {
                    let ValueType::Struct(struct_id) = ty else {
                        return Err(at(span, "field access requires a struct value"));
                    };
                    let field = self.structs[struct_id]
                        .fields
                        .iter()
                        .find(|field| field.name == *name)
                        .ok_or_else(|| {
                            at(
                                span,
                                format!(
                                    "struct `{}` has no field `{name}`",
                                    self.structs[struct_id].name
                                ),
                            )
                        })?;
                    let field_type = field.ty.clone();
                    let field_span = field.span;
                    ty = self.resolve_type(&field_type, field_span)?;
                }
                BorrowProjection::Index(_) => {
                    ty = match ty {
                        ValueType::Array(index) => {
                            ValueType::from(self.array_shapes[index].element)
                        }
                        ValueType::Vector(index) => {
                            ValueType::from(self.vector_shapes[index].element)
                        }
                        _ => return Err(at(span, "indexing requires an array (or vector)")),
                    };
                }
            }
        }
        Ok(ty)
    }

    fn provenance_may_overlap(left: &ReferenceProvenance, right: &ReferenceProvenance) -> bool {
        if left.is_unknown() || right.is_unknown() {
            return true;
        }
        for left_source in &left.sources {
            for right_source in &right.sources {
                match (left_source.place(), right_source.place()) {
                    (Some(left), Some(right)) => {
                        if left.overlaps(right) {
                            return true;
                        }
                    }
                    _ => return true,
                }
            }
        }
        false
    }

    fn reject_overlapping_reference_arguments_with(
        &mut self,
        args: &[Expr],
        params: &[ValueType],
        origins: &[Option<ReferenceProvenance>],
        conservative: bool,
    ) -> Result<(), Diagnostic> {
        debug_assert_eq!(args.len(), origins.len());
        for later in 0..args.len() {
            let later_expected = params.get(later).copied();
            let later_mutable = matches!(later_expected, Some(ValueType::MutableReference(_)));
            let later_reference = matches!(
                later_expected,
                Some(ValueType::Reference(_) | ValueType::MutableReference(_))
            );
            if !later_reference {
                continue;
            }
            for earlier in 0..later {
                let earlier_expected = params.get(earlier).copied();
                let earlier_mutable =
                    matches!(earlier_expected, Some(ValueType::MutableReference(_)));
                let earlier_reference = matches!(
                    earlier_expected,
                    Some(ValueType::Reference(_) | ValueType::MutableReference(_))
                );
                if !earlier_reference || (!earlier_mutable && !later_mutable) {
                    continue;
                }
                let overlaps = match (&origins[earlier], &origins[later]) {
                    (Some(earlier), Some(later))
                        if conservative || !(earlier.is_unknown() || later.is_unknown()) =>
                    {
                        Self::provenance_may_overlap(earlier, later)
                    }
                    (Some(_), Some(_)) => false,
                    _ => conservative,
                };
                if overlaps {
                    return Err(at(
                        span_of_expr(&args[later]),
                        "overlapping mutable references cannot be passed to the same call",
                    ));
                }
            }
        }
        Ok(())
    }

    pub(super) fn reject_known_overlapping_reference_arguments(
        &mut self,
        args: &[Expr],
        params: &[ValueType],
    ) -> Result<(), Diagnostic> {
        let origins = args
            .iter()
            .map(|argument| self.reference_origin(argument))
            .collect::<Result<Vec<_>, _>>()?;
        self.reject_overlapping_reference_arguments_with(args, params, &origins, false)
    }

    pub(super) fn reject_overlapping_reference_arguments(
        &mut self,
        args: &[Expr],
        params: &[ValueType],
        origins: &[Option<ReferenceProvenance>],
    ) -> Result<(), Diagnostic> {
        self.reject_overlapping_reference_arguments_with(args, params, origins, true)
    }
}

fn constant_i64(expr: &Expr) -> Option<i64> {
    match expr {
        Expr::Integer(value, _) if *value <= i64::MAX as u64 => Some(*value as i64),
        Expr::Unary {
            op: UnaryOp::Negate,
            value,
            ..
        } => match value.as_ref() {
            Expr::Integer(value, _) if *value <= i64::MAX as u64 => Some(-(*value as i64)),
            Expr::Integer(value, _) if *value == (i64::MAX as u64) + 1 => Some(i64::MIN),
            _ => None,
        },
        _ => None,
    }
}
