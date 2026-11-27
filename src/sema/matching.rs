use super::*;

/// A pattern with module and type information resolved away. Coverage analysis
/// works on rows of these values, so it never has to look at source syntax.
#[derive(Debug, Clone, PartialEq)]
enum PatternRow {
    Wild,
    Bool(bool),
    I64(i64),
    F32(f32),
    F64(f64),
    Variant {
        enum_id: usize,
        variant_id: usize,
        children: Vec<PatternRow>,
    },
    /// Tuples, fixed arrays, and structs all have exactly one constructor.
    Product(Vec<PatternRow>),
}

#[derive(Debug, Clone, PartialEq)]
enum Constructor {
    Bool(bool),
    I64(i64),
    F32(f32),
    F64(f64),
    Variant(usize, usize),
    Product,
}

/// Refuse pathological pattern matrices rather than guess at coverage.
const MATRIX_BUDGET: usize = 500_000;

impl Checker {
    pub(super) fn check_match_expr(
        &mut self,
        value: &Expr,
        arms: &[MatchArm],
        span: Span,
    ) -> Result<ValueType, Diagnostic> {
        let value_type = self.check_expr(value, false)?;
        if !Self::is_matchable(value_type) {
            return Err(at(
                span,
                format!(
                    "match requires an enum or destructurable aggregate value, got {}",
                    self.type_name(value_type)
                ),
            ));
        }
        let mut rows = Vec::with_capacity(arms.len());
        let mut result_type = None;
        let mut result_origins = Vec::new();
        let mut budget = MATRIX_BUDGET;
        for (index, arm) in arms.iter().enumerate() {
            let mut bindings = Vec::new();
            let mut seen = std::collections::HashSet::new();
            let row = self.build_pattern_row(
                &arm.pattern,
                value_type,
                arm.span,
                &mut bindings,
                &mut seen,
            )?;
            let useful = self
                .is_useful(
                    &rows,
                    std::slice::from_ref(&row),
                    &[value_type],
                    &mut budget,
                )
                .map_err(|error| at(span, error.message))?;
            if !useful {
                return Err(self.unreachable_arm(arms, index, value_type, &row, &rows));
            }
            let (actual, origin) = self.check_match_arm_body(arm, bindings)?;
            result_origins.push(origin);
            if let Some(expected) = result_type {
                self.expect_type(actual, expected, arm.span, "match arm result")?;
            } else {
                result_type = Some(actual);
            }
            rows.push(vec![row]);
        }
        let exhaustive = !self
            .is_useful(
                &rows,
                &[PatternRow::Wild],
                std::slice::from_ref(&value_type),
                &mut budget,
            )
            .map_err(|error| at(span, error.message))?;
        if !exhaustive {
            return Err(self
                .non_exhaustive_match(value_type, &rows, arms, span, &mut budget)
                .map_err(|error| at(span, error.message))?);
        }
        let result_type =
            result_type.ok_or_else(|| at(span, "match expression must have at least one arm"))?;
        self.record_reference_join(span, result_type, result_origins, "`match`")?;
        Ok(result_type)
    }

    fn is_matchable(ty: ValueType) -> bool {
        matches!(
            ty,
            ValueType::Bool
                | ValueType::I64
                | ValueType::F32
                | ValueType::F64
                | ValueType::Enum(_)
                | ValueType::Struct(_)
                | ValueType::Tuple(_)
                | ValueType::Array(_)
        )
    }

    fn check_match_arm_body(
        &mut self,
        arm: &MatchArm,
        bindings: Vec<(String, ValueType)>,
    ) -> Result<(ValueType, ReferenceProvenance), Diagnostic> {
        self.scopes
            .push(bindings.into_iter().collect::<HashMap<_, _>>());
        self.immutable_scopes.push(std::collections::HashSet::new());
        self.loop_scopes.push(std::collections::HashSet::new());
        self.push_loan_scope();
        let result = self.check_block_result(&arm.body, arm.span);
        self.scopes.pop();
        self.immutable_scopes.pop();
        self.loop_scopes.pop();
        self.pop_loan_scope();
        result
    }

    /// Validate a pattern against the type at its position and resolve it into a
    /// coverage row. Binding names are collected across the whole arm.
    fn build_pattern_row(
        &mut self,
        pattern: &MatchPattern,
        ty: ValueType,
        span: Span,
        bindings: &mut Vec<(String, ValueType)>,
        seen: &mut std::collections::HashSet<String>,
    ) -> Result<PatternRow, Diagnostic> {
        Ok(match pattern {
            MatchPattern::Wildcard => PatternRow::Wild,
            MatchPattern::Binding(name) => {
                if !seen.insert(name.clone()) {
                    return Err(at(
                        span,
                        format!("`{name}` is already declared in this scope"),
                    ));
                }
                bindings.push((name.clone(), ty));
                PatternRow::Wild
            }
            MatchPattern::Bool(value) => {
                if ty != ValueType::Bool {
                    return Err(self.scalar_pattern_mismatch(ty, span));
                }
                PatternRow::Bool(*value)
            }
            MatchPattern::I64(value) => {
                if ty != ValueType::I64 {
                    return Err(self.scalar_pattern_mismatch(ty, span));
                }
                PatternRow::I64(*value)
            }
            MatchPattern::F32(value) => {
                if ty != ValueType::F32 {
                    return Err(self.scalar_pattern_mismatch(ty, span));
                }
                PatternRow::F32(*value)
            }
            MatchPattern::F64(value) => {
                if ty != ValueType::F64 {
                    return Err(self.scalar_pattern_mismatch(ty, span));
                }
                PatternRow::F64(*value)
            }
            MatchPattern::Tuple(patterns) => {
                let ValueType::Tuple(tuple_id) = ty else {
                    return Err(at(
                        span,
                        format!("tuple pattern cannot match {}", self.type_name(ty)),
                    ));
                };
                let types = self.tuple_shapes[tuple_id].clone();
                if patterns.len() != types.len() {
                    return Err(at(
                        span,
                        format!(
                            "tuple pattern has {} element(s), expected {}",
                            patterns.len(),
                            types.len()
                        ),
                    ));
                }
                let mut children = Vec::with_capacity(types.len());
                for (pattern, element) in patterns.iter().zip(types) {
                    children.push(self.build_pattern_row(pattern, element, span, bindings, seen)?);
                }
                PatternRow::Product(children)
            }
            MatchPattern::Array(patterns) => {
                let ValueType::Array(array_id) = ty else {
                    return Err(at(
                        span,
                        format!("array pattern cannot match {}", self.type_name(ty)),
                    ));
                };
                let shape = self.array_shapes[array_id];
                if patterns.len() != shape.length as usize {
                    return Err(at(
                        span,
                        format!(
                            "array pattern has {} element(s), expected {}",
                            patterns.len(),
                            shape.length
                        ),
                    ));
                }
                let element = ValueType::from(shape.element);
                let mut children = Vec::with_capacity(patterns.len());
                for pattern in patterns {
                    children.push(self.build_pattern_row(pattern, element, span, bindings, seen)?);
                }
                PatternRow::Product(children)
            }
            MatchPattern::Struct {
                name, fields, rest, ..
            } => {
                let ValueType::Struct(struct_id) = ty else {
                    return Err(at(
                        span,
                        format!("struct pattern cannot match {}", self.type_name(ty)),
                    ));
                };
                let declaration = self.structs[struct_id].clone();
                if name != &declaration.name {
                    return Err(at(
                        span,
                        format!(
                            "struct pattern uses `{name}`, expected `{}`",
                            declaration.name
                        ),
                    ));
                }
                if declaration.module != self.module_context && !declaration.is_public {
                    return Err(at(span, format!("struct `{name}` is private")));
                }
                let mut resolved = HashMap::new();
                for (field_name, pattern, field_span) in fields {
                    if resolved.contains_key(field_name.as_str()) {
                        return Err(at(
                            *field_span,
                            format!(
                                "struct field `{field_name}` appears more than once in pattern"
                            ),
                        ));
                    }
                    let field = declaration
                        .fields
                        .iter()
                        .find(|field| field.name == *field_name)
                        .ok_or_else(|| {
                            at(
                                *field_span,
                                format!("struct `{name}` has no field `{field_name}`"),
                            )
                        })?;
                    let field_type = self.resolve_type(&field.ty, *field_span)?;
                    let row =
                        self.build_pattern_row(pattern, field_type, *field_span, bindings, seen)?;
                    resolved.insert(field_name.clone(), row);
                }
                if !rest && resolved.len() != declaration.fields.len() {
                    let missing = declaration
                        .fields
                        .iter()
                        .filter(|field| !resolved.contains_key(&field.name))
                        .map(|field| field.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    return Err(at(
                        span,
                        format!(
                            "struct pattern for `{name}` is missing field(s): {missing}; add `..` to ignore remaining fields"
                        ),
                    ));
                }
                let children = declaration
                    .fields
                    .iter()
                    .map(|field| resolved.remove(&field.name).unwrap_or(PatternRow::Wild))
                    .collect();
                PatternRow::Product(children)
            }
            MatchPattern::Variant {
                enum_name,
                variant_name,
                bindings: payload,
            } => {
                let ValueType::Enum(enum_id) = ty else {
                    return Err(at(span, "enum variant patterns require an enum value"));
                };
                let declaration = self.enums[enum_id].clone();
                if enum_name != &declaration.name {
                    return Err(at(
                        span,
                        format!(
                            "match pattern uses enum `{enum_name}`, expected `{}`",
                            declaration.name
                        ),
                    ));
                }
                let variant_id = declaration
                    .variants
                    .iter()
                    .position(|variant| variant.name == *variant_name)
                    .ok_or_else(|| {
                        at(
                            span,
                            format!(
                                "enum `{}` has no variant `{variant_name}`",
                                declaration.name
                            ),
                        )
                    })?;
                let variant = declaration.variants[variant_id].clone();
                let patterns = match (payload, variant.payload.is_empty()) {
                    (None, true) => &[][..],
                    (None, false) => {
                        return Err(at(
                            span,
                            format!(
                                "match arm `{}::{variant_name}` must bind its payload",
                                declaration.name
                            ),
                        ));
                    }
                    (Some(_), true) => {
                        return Err(at(
                            span,
                            format!(
                                "match arm `{}::{variant_name}` has no payload to bind or ignore",
                                declaration.name
                            ),
                        ));
                    }
                    (Some(patterns), false) if patterns.len() != variant.payload.len() => {
                        return Err(at(
                            span,
                            format!(
                                "match arm `{}::{variant_name}` expects {} payload binding(s), got {}",
                                declaration.name,
                                variant.payload.len(),
                                patterns.len()
                            ),
                        ));
                    }
                    (Some(patterns), false) => patterns.as_slice(),
                };
                let mut children = Vec::with_capacity(patterns.len());
                for (pattern, payload_type) in patterns.iter().zip(&variant.payload) {
                    let payload_type = self.resolve_type(payload_type, span)?;
                    children.push(self.build_pattern_row(
                        pattern,
                        payload_type,
                        span,
                        bindings,
                        seen,
                    )?);
                }
                PatternRow::Variant {
                    enum_id,
                    variant_id,
                    children,
                }
            }
            MatchPattern::ImportedVariant { .. } => {
                return Err(at(
                    span,
                    "imported enum variant pattern reached semantic analysis",
                ));
            }
        })
    }

    fn scalar_pattern_mismatch(&self, ty: ValueType, span: Span) -> Diagnostic {
        at(
            span,
            format!(
                "match pattern type does not match {} value",
                self.type_name(ty)
            ),
        )
    }

    /// Maranget-style usefulness: is there a value matched by `v` but not by any
    /// row of `matrix`?
    fn is_useful(
        &mut self,
        matrix: &[Vec<PatternRow>],
        v: &[PatternRow],
        types: &[ValueType],
        budget: &mut usize,
    ) -> Result<bool, Diagnostic> {
        let matrix = row_refs(matrix);
        self.is_useful_rows(&matrix, v, types, budget)
    }

    fn is_useful_rows(
        &mut self,
        matrix: &[&[PatternRow]],
        v: &[PatternRow],
        types: &[ValueType],
        budget: &mut usize,
    ) -> Result<bool, Diagnostic> {
        if *budget == 0 {
            return Err(Diagnostic::new(
                "match pattern analysis exceeded its complexity limit".to_string(),
            ));
        }
        *budget -= 1;
        if types.is_empty() {
            return Ok(matrix.is_empty());
        }
        let ty = types[0];
        match &v[0] {
            PatternRow::Wild => {
                if matrix
                    .iter()
                    .all(|row| matches!(row.first(), Some(PatternRow::Wild)))
                {
                    let default = default_matrix(matrix);
                    return self.is_useful_rows(&default, &v[1..], &types[1..], budget);
                }
                if let Some(constructors) = self.complete_constructors(ty, matrix) {
                    for constructor in constructors {
                        let child_types = self.constructor_child_types(&constructor, ty)?;
                        let mut head = vec![PatternRow::Wild; child_types.len()];
                        head.extend_from_slice(&v[1..]);
                        let mut columns = child_types.clone();
                        columns.extend_from_slice(&types[1..]);
                        let specialized = self.specialize(matrix, &constructor, child_types.len());
                        let specialized_refs = row_refs(&specialized);
                        if self.is_useful_rows(&specialized_refs, &head, &columns, budget)? {
                            return Ok(true);
                        }
                    }
                    Ok(false)
                } else {
                    let default = default_matrix(matrix);
                    self.is_useful_rows(&default, &v[1..], &types[1..], budget)
                }
            }
            row => {
                let constructor = constructor_of(row).ok_or_else(|| {
                    Diagnostic::new("internal error: pattern row has no constructor".to_string())
                })?;
                let child_types = self.constructor_child_types(&constructor, ty)?;
                let child_count = row_children(row).len();
                let mut head = row_children(row).to_vec();
                head.extend_from_slice(&v[1..]);
                let mut columns = child_types;
                columns.extend_from_slice(&types[1..]);
                let specialized = self.specialize(matrix, &constructor, child_count);
                let specialized_refs = row_refs(&specialized);
                self.is_useful_rows(&specialized_refs, &head, &columns, budget)
            }
        }
    }

    /// Render the first value matched by no row, for exhaustiveness diagnostics.
    fn witness(
        &mut self,
        matrix: &[Vec<PatternRow>],
        types: &[ValueType],
        budget: &mut usize,
    ) -> Result<Option<Vec<String>>, Diagnostic> {
        let matrix = row_refs(matrix);
        self.witness_rows(&matrix, types, budget)
    }

    fn witness_rows(
        &mut self,
        matrix: &[&[PatternRow]],
        types: &[ValueType],
        budget: &mut usize,
    ) -> Result<Option<Vec<String>>, Diagnostic> {
        if *budget == 0 {
            return Err(Diagnostic::new(
                "match pattern analysis exceeded its complexity limit".to_string(),
            ));
        }
        *budget -= 1;
        if types.is_empty() {
            return Ok(matrix.is_empty().then(Vec::new));
        }
        if matrix
            .iter()
            .all(|row| matches!(row.first(), Some(PatternRow::Wild)))
        {
            let mut default = default_matrix(matrix);
            let mut skipped = 1;
            while !types[skipped..].is_empty()
                && default
                    .iter()
                    .all(|row| matches!(row.first(), Some(PatternRow::Wild)))
            {
                default = default_matrix(&default);
                skipped += 1;
            }
            if let Some(mut witness) = self.witness_rows(&default, &types[skipped..], budget)? {
                for _ in 0..skipped {
                    witness.insert(0, "_".to_owned());
                }
                return Ok(Some(witness));
            }
            return Ok(None);
        }
        let ty = types[0];
        if let Some(constructors) = self.all_constructors(ty) {
            for constructor in constructors {
                let child_types = self.constructor_child_types(&constructor, ty)?;
                let specialized = self.specialize(matrix, &constructor, child_types.len());
                let specialized_refs = row_refs(&specialized);
                let mut columns = child_types.clone();
                columns.extend_from_slice(&types[1..]);
                if let Some(mut witness) = self.witness_rows(&specialized_refs, &columns, budget)? {
                    let rest = witness.split_off(child_types.len());
                    let rendered = self.render_constructor(&constructor, ty, &witness);
                    let mut result = vec![rendered];
                    result.extend(rest);
                    return Ok(Some(result));
                }
            }
            return Ok(None);
        }
        let default = default_matrix(matrix);
        let mut seen = Vec::new();
        for row in matrix {
            if let Some(constructor) = constructor_of(&row[0])
                && !seen.contains(&constructor)
            {
                seen.push(constructor);
            }
        }
        if let Some(mut witness) = self.witness_rows(&default, &types[1..], budget)? {
            witness.insert(0, "_".to_owned());
            return Ok(Some(witness));
        }
        for constructor in seen {
            let specialized = self.specialize(matrix, &constructor, 0);
            let specialized_refs = row_refs(&specialized);
            let columns = types[1..].to_vec();
            if let Some(mut witness) = self.witness_rows(&specialized_refs, &columns, budget)? {
                let rendered = self.render_constructor(&constructor, ty, &[]);
                witness.insert(0, rendered);
                return Ok(Some(witness));
            }
        }
        Ok(None)
    }

    fn specialize(
        &self,
        matrix: &[&[PatternRow]],
        constructor: &Constructor,
        child_count: usize,
    ) -> Vec<Vec<PatternRow>> {
        let mut result = Vec::with_capacity(matrix.len());
        for row in matrix {
            match &row[0] {
                PatternRow::Wild => {
                    let mut expanded = vec![PatternRow::Wild; child_count];
                    expanded.extend_from_slice(&row[1..]);
                    result.push(expanded);
                }
                other if constructor_of(other) == Some(constructor.clone()) => {
                    let mut expanded = row_children(other).to_vec();
                    expanded.extend_from_slice(&row[1..]);
                    result.push(expanded);
                }
                _ => {}
            }
        }
        result
    }

    fn complete_constructors(
        &self,
        ty: ValueType,
        matrix: &[&[PatternRow]],
    ) -> Option<Vec<Constructor>> {
        match ty {
            ValueType::Bool => {
                let mut seen = Vec::new();
                for row in matrix {
                    if let PatternRow::Bool(value) = &row[0]
                        && !seen.contains(value)
                    {
                        seen.push(*value);
                    }
                }
                (seen.len() == 2).then(|| vec![Constructor::Bool(false), Constructor::Bool(true)])
            }
            ValueType::Enum(enum_id) => {
                let mut seen = Vec::new();
                for row in matrix {
                    if let Some(Constructor::Variant(_, variant_id)) = constructor_of(&row[0])
                        && !seen.contains(&variant_id)
                    {
                        seen.push(variant_id);
                    }
                }
                (seen.len() == self.enums[enum_id].variants.len()).then(|| {
                    (0..self.enums[enum_id].variants.len())
                        .map(|variant_id| Constructor::Variant(enum_id, variant_id))
                        .collect()
                })
            }
            ValueType::Tuple(_) | ValueType::Array(_) | ValueType::Struct(_) => matrix
                .iter()
                .any(|row| matches!(row[0], PatternRow::Product(_)))
                .then(|| vec![Constructor::Product]),
            ValueType::I64 | ValueType::F32 | ValueType::F64 => None,
            _ => None,
        }
    }

    fn all_constructors(&self, ty: ValueType) -> Option<Vec<Constructor>> {
        match ty {
            ValueType::Bool => Some(vec![Constructor::Bool(false), Constructor::Bool(true)]),
            ValueType::Enum(enum_id) => Some(
                (0..self.enums[enum_id].variants.len())
                    .map(|variant_id| Constructor::Variant(enum_id, variant_id))
                    .collect(),
            ),
            ValueType::Tuple(_) | ValueType::Array(_) | ValueType::Struct(_) => {
                Some(vec![Constructor::Product])
            }
            ValueType::I64 | ValueType::F32 | ValueType::F64 => None,
            _ => None,
        }
    }

    fn constructor_child_types(
        &mut self,
        constructor: &Constructor,
        ty: ValueType,
    ) -> Result<Vec<ValueType>, Diagnostic> {
        Ok(match constructor {
            Constructor::Bool(_)
            | Constructor::I64(_)
            | Constructor::F32(_)
            | Constructor::F64(_) => Vec::new(),
            Constructor::Variant(enum_id, variant_id) => {
                let declaration = self.enums[*enum_id].clone();
                let variant = &declaration.variants[*variant_id];
                let mut types = Vec::with_capacity(variant.payload.len());
                for payload in &variant.payload {
                    types.push(self.resolve_type(payload, variant.span)?);
                }
                types
            }
            Constructor::Product => match ty {
                ValueType::Tuple(tuple_id) => self.tuple_shapes[tuple_id].clone(),
                ValueType::Array(array_id) => {
                    let shape = self.array_shapes[array_id];
                    vec![ValueType::from(shape.element); shape.length as usize]
                }
                ValueType::Struct(struct_id) => {
                    let declaration = self.structs[struct_id].clone();
                    let mut types = Vec::with_capacity(declaration.fields.len());
                    for field in &declaration.fields {
                        types.push(self.resolve_type(&field.ty, field.span)?);
                    }
                    types
                }
                _ => {
                    return Err(Diagnostic::new(
                        "internal error: product pattern has no aggregate type".to_string(),
                    ));
                }
            },
        })
    }

    fn render_constructor(
        &self,
        constructor: &Constructor,
        ty: ValueType,
        children: &[String],
    ) -> String {
        match constructor {
            Constructor::Bool(value) => value.to_string(),
            Constructor::I64(value) => value.to_string(),
            Constructor::F32(value) => format!("{value}f32"),
            Constructor::F64(value) => value.to_string(),
            Constructor::Variant(enum_id, variant_id) => {
                let declaration = &self.enums[*enum_id];
                let variant = &declaration.variants[*variant_id];
                if children.is_empty() {
                    format!("{}::{}", declaration.name, variant.name)
                } else {
                    format!(
                        "{}::{}({})",
                        declaration.name,
                        variant.name,
                        children.join(", ")
                    )
                }
            }
            Constructor::Product => match ty {
                ValueType::Tuple(_) => {
                    let trailing_comma = if children.len() == 1 { "," } else { "" };
                    format!("({}{trailing_comma})", children.join(", "))
                }
                ValueType::Array(_) => format!("[{}]", children.join(", ")),
                ValueType::Struct(struct_id) => {
                    let declaration = &self.structs[struct_id];
                    let fields = declaration
                        .fields
                        .iter()
                        .zip(children)
                        .map(|(field, child)| format!("{}: {child}", field.name))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("{} {{ {fields} }}", declaration.name)
                }
                _ => children.join(", "),
            },
        }
    }

    fn unreachable_arm(
        &self,
        arms: &[MatchArm],
        index: usize,
        ty: ValueType,
        row: &PatternRow,
        prior: &[Vec<PatternRow>],
    ) -> Diagnostic {
        let arm = &arms[index];
        let catch_all = arms[..index].iter().any(|arm| {
            matches!(
                arm.pattern,
                MatchPattern::Wildcard | MatchPattern::Binding(_)
            )
        });
        let is_enum = matches!(ty, ValueType::Enum(_));
        if matches!(arm.pattern, MatchPattern::Wildcard) {
            if catch_all {
                return at(
                    arm.span,
                    if is_enum {
                        "duplicate wildcard match arm"
                    } else {
                        "unreachable match arm after wildcard"
                    },
                );
            }
            return at(
                arm.span,
                if is_enum {
                    "unreachable wildcard match arm after all variants are covered"
                } else {
                    "unreachable wildcard match arm after all values are covered"
                },
            );
        }
        if catch_all {
            return at(arm.span, "unreachable match arm after wildcard");
        }
        if prior
            .iter()
            .any(|earlier| earlier.len() == 1 && earlier[0] == *row)
        {
            return match row {
                PatternRow::Bool(value) => {
                    at(arm.span, format!("duplicate match arm for `{value}`"))
                }
                PatternRow::I64(value) => {
                    at(arm.span, format!("duplicate match arm for `{value}`"))
                }
                PatternRow::F32(value) => {
                    at(arm.span, format!("duplicate match arm for `{value}`"))
                }
                PatternRow::F64(value) => {
                    at(arm.span, format!("duplicate match arm for `{value}`"))
                }
                PatternRow::Variant { variant_id, .. } if is_enum => {
                    let ValueType::Enum(enum_id) = ty else {
                        unreachable!()
                    };
                    let declaration = &self.enums[enum_id];
                    at(
                        arm.span,
                        format!(
                            "duplicate match arm for `{}::{}`",
                            declaration.name, declaration.variants[*variant_id].name
                        ),
                    )
                }
                _ => at(arm.span, "duplicate match arm"),
            };
        }
        at(arm.span, "unreachable match arm")
    }

    fn non_exhaustive_match(
        &mut self,
        ty: ValueType,
        rows: &[Vec<PatternRow>],
        arms: &[MatchArm],
        span: Span,
        budget: &mut usize,
    ) -> Result<Diagnostic, Diagnostic> {
        let report_span = arms.last().map(|arm| arm.span).unwrap_or(span);
        match ty {
            ValueType::Bool => {
                let missing_false =
                    self.is_useful(rows, &[PatternRow::Bool(false)], &[ty], budget)?;
                let missing = if missing_false { "false" } else { "true" };
                return Ok(at(
                    report_span,
                    format!("non-exhaustive match on `bool`; missing value: {missing}"),
                ));
            }
            ValueType::I64 | ValueType::F32 | ValueType::F64 => {
                return Ok(at(
                    span,
                    format!(
                        "non-exhaustive match on `{}`; add a wildcard (`_`) arm",
                        self.type_name(ty)
                    ),
                ));
            }
            ValueType::Enum(enum_id) => {
                let mut covered = Vec::new();
                for row in rows {
                    if let PatternRow::Variant { variant_id, .. } = &row[0]
                        && !covered.contains(variant_id)
                    {
                        covered.push(*variant_id);
                    }
                }
                if covered.len() != self.enums[enum_id].variants.len() {
                    let missing = self.enums[enum_id]
                        .variants
                        .iter()
                        .enumerate()
                        .filter(|(index, _)| !covered.contains(index))
                        .map(|(_, variant)| variant.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    return Ok(at(
                        span,
                        format!(
                            "non-exhaustive match on `{}`; missing variant(s): {missing}",
                            self.enums[enum_id].name
                        ),
                    ));
                }
                if let Some(witness) = self.witness(rows, &[ty], budget)? {
                    return Ok(at(
                        span,
                        format!(
                            "non-exhaustive match on `{}`; missing case: {}",
                            self.enums[enum_id].name,
                            witness.join(", ")
                        ),
                    ));
                }
            }
            _ => {}
        }
        if let Some(witness) = self.witness(rows, &[ty], budget)? {
            return Ok(at(
                span,
                format!(
                    "non-exhaustive match on `{}`; missing case: {}",
                    self.type_name(ty),
                    witness.join(", ")
                ),
            ));
        }
        Ok(at(
            span,
            format!(
                "non-exhaustive match on `{}`; add a wildcard (`_`) arm",
                self.type_name(ty)
            ),
        ))
    }

    /// Irrefutable `let`/`const` patterns keep their previous, narrower rules.
    pub(super) fn check_pattern(
        &mut self,
        pattern: &MatchPattern,
        ty: ValueType,
        span: Span,
        bindings: &mut Vec<(String, ValueType)>,
        seen: &mut std::collections::HashSet<String>,
    ) -> Result<(), Diagnostic> {
        match pattern {
            MatchPattern::Wildcard => Ok(()),
            MatchPattern::Binding(name) => {
                if !seen.insert(name.clone()) {
                    return Err(at(
                        span,
                        format!("`{name}` is already declared in this scope"),
                    ));
                }
                bindings.push((name.clone(), ty));
                Ok(())
            }
            MatchPattern::Tuple(patterns) => {
                let ValueType::Tuple(tuple_id) = ty else {
                    return Err(at(
                        span,
                        format!("tuple pattern cannot match {}", self.type_name(ty)),
                    ));
                };
                let types = self.tuple_shapes[tuple_id].clone();
                if patterns.len() != types.len() {
                    return Err(at(
                        span,
                        format!(
                            "tuple pattern has {} element(s), expected {}",
                            patterns.len(),
                            types.len()
                        ),
                    ));
                }
                for (pattern, ty) in patterns.iter().zip(types) {
                    self.check_pattern(pattern, ty, span, bindings, seen)?;
                }
                Ok(())
            }
            MatchPattern::Array(patterns) => {
                let ValueType::Array(array_id) = ty else {
                    return Err(at(
                        span,
                        format!("array pattern cannot match {}", self.type_name(ty)),
                    ));
                };
                let shape = self.array_shapes[array_id];
                if patterns.len() != shape.length as usize {
                    return Err(at(
                        span,
                        format!(
                            "array pattern has {} element(s), expected {}",
                            patterns.len(),
                            shape.length
                        ),
                    ));
                }
                for pattern in patterns {
                    self.check_pattern(
                        pattern,
                        ValueType::from(shape.element),
                        span,
                        bindings,
                        seen,
                    )?;
                }
                Ok(())
            }
            MatchPattern::Struct {
                name, fields, rest, ..
            } => {
                let ValueType::Struct(struct_id) = ty else {
                    return Err(at(
                        span,
                        format!("struct pattern cannot match {}", self.type_name(ty)),
                    ));
                };
                let declaration = self.structs[struct_id].clone();
                if name != &declaration.name {
                    return Err(at(
                        span,
                        format!(
                            "struct pattern uses `{name}`, expected `{}`",
                            declaration.name
                        ),
                    ));
                }
                if declaration.module != self.module_context && !declaration.is_public {
                    return Err(at(span, format!("struct `{name}` is private")));
                }
                let mut seen_fields = std::collections::HashSet::new();
                for (field_name, pattern, field_span) in fields {
                    if !seen_fields.insert(field_name.as_str()) {
                        return Err(at(
                            *field_span,
                            format!(
                                "struct field `{field_name}` appears more than once in pattern"
                            ),
                        ));
                    }
                    let field = declaration
                        .fields
                        .iter()
                        .find(|field| field.name == *field_name)
                        .ok_or_else(|| {
                            at(
                                *field_span,
                                format!("struct `{name}` has no field `{field_name}`"),
                            )
                        })?;
                    let field_type = self.resolve_type(&field.ty, *field_span)?;
                    self.check_pattern(pattern, field_type, *field_span, bindings, seen)?;
                }
                if !rest && fields.len() != declaration.fields.len() {
                    let missing = declaration
                        .fields
                        .iter()
                        .filter(|field| !seen_fields.contains(field.name.as_str()))
                        .map(|field| field.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    return Err(at(
                        span,
                        format!(
                            "struct pattern for `{name}` is missing field(s): {missing}; add `..` to ignore remaining fields"
                        ),
                    ));
                }
                Ok(())
            }
            MatchPattern::Variant { .. } => Err(at(
                span,
                "enum variant pattern is not valid in this pattern position",
            )),
            MatchPattern::ImportedVariant { .. } => Err(at(
                span,
                "imported enum variant pattern reached semantic analysis",
            )),
            MatchPattern::Bool(_)
            | MatchPattern::I64(_)
            | MatchPattern::F32(_)
            | MatchPattern::F64(_) => Err(at(
                span,
                "literal patterns are only supported for scalar match expressions",
            )),
        }
    }
}

fn constructor_of(row: &PatternRow) -> Option<Constructor> {
    match row {
        PatternRow::Wild => None,
        PatternRow::Bool(value) => Some(Constructor::Bool(*value)),
        PatternRow::I64(value) => Some(Constructor::I64(*value)),
        PatternRow::F32(value) => Some(Constructor::F32(*value)),
        PatternRow::F64(value) => Some(Constructor::F64(*value)),
        PatternRow::Variant {
            enum_id,
            variant_id,
            ..
        } => Some(Constructor::Variant(*enum_id, *variant_id)),
        PatternRow::Product(_) => Some(Constructor::Product),
    }
}

fn row_children(row: &PatternRow) -> &[PatternRow] {
    match row {
        PatternRow::Variant { children, .. } | PatternRow::Product(children) => children,
        _ => &[],
    }
}

fn row_refs(matrix: &[Vec<PatternRow>]) -> Vec<&[PatternRow]> {
    matrix.iter().map(Vec::as_slice).collect()
}

fn default_matrix<'a>(matrix: &[&'a [PatternRow]]) -> Vec<&'a [PatternRow]> {
    matrix
        .iter()
        .filter(|row| matches!(row.first(), Some(PatternRow::Wild)))
        .map(|row| &row[1..])
        .collect()
}
