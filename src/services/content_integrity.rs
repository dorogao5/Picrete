use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::db::models::{TaskType, TaskVariant};

const SCORE_EPSILON: f64 = 0.01;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ContentIssue {
    pub(crate) location: String,
    pub(crate) message: String,
}

impl ContentIssue {
    fn new(location: impl Into<String>, message: impl Into<String>) -> Self {
        Self { location: location.into(), message: message.into() }
    }
}

pub(crate) fn validate_exam_content(
    course_id: &str,
    task_types: &[TaskType],
    variants: &[TaskVariant],
) -> Vec<ContentIssue> {
    let mut issues = Vec::new();
    let mut variants_by_task_type = HashMap::<&str, Vec<&TaskVariant>>::new();
    for variant in variants {
        variants_by_task_type.entry(variant.task_type_id.as_str()).or_default().push(variant);
    }

    let mut order_indexes = HashSet::new();
    for task_type in task_types {
        let location = format!("task_type:{}", task_type.id);
        validate_required_text(&task_type.title, &location, "title", &mut issues);
        validate_required_text(&task_type.description, &location, "description", &mut issues);

        if !task_type.max_score.is_finite() || task_type.max_score <= 0.0 {
            issues.push(ContentIssue::new(&location, "max_score must be a finite positive number"));
        }
        if !order_indexes.insert(task_type.order_index) {
            issues.push(ContentIssue::new(
                &location,
                format!("order_index {} is duplicated", task_type.order_index),
            ));
        }

        validate_rubric(&task_type.rubric.0, task_type.max_score, &location, &mut issues);
        validate_formulas(&task_type.formulas.0, &location, &mut issues);
        validate_units(&task_type.units.0, &location, &mut issues);
        validate_validation_rules(
            &task_type.validation_rules.0,
            task_type.max_score,
            &location,
            &mut issues,
        );

        let task_variants =
            variants_by_task_type.get(task_type.id.as_str()).map(Vec::as_slice).unwrap_or_default();
        if task_variants.is_empty() {
            issues.push(ContentIssue::new(&location, "at least one variant is required"));
            continue;
        }

        let bank_snapshot = task_type
            .validation_rules
            .0
            .pointer("/source_ref/source_code")
            .and_then(Value::as_str)
            .is_some();
        for variant in task_variants {
            validate_variant(course_id, variant, bank_snapshot, &mut issues);
        }
    }

    issues
}

pub(crate) fn validate_bank_item(
    number: &str,
    text: &str,
    answer: Option<&str>,
    solution: Option<&str>,
    has_answer: bool,
    has_images: bool,
) -> Vec<ContentIssue> {
    let location = format!("bank_item:{number}");
    let mut issues = Vec::new();
    validate_required_text(text, &location, "text", &mut issues);
    validate_content_text(text, &location, "text", &mut issues);
    validate_required_illustration(text, has_images, &location, &mut issues);

    let normalized_answer = answer.map(str::trim).filter(|value| !value.is_empty());
    let normalized_solution = solution.map(str::trim).filter(|value| !value.is_empty());
    if (!has_answer || normalized_answer.is_none()) && normalized_solution.is_none() {
        issues.push(ContentIssue::new(
            &location,
            "a non-empty reference answer or solution is required before the item can be added to an assessment",
        ));
    }
    if let Some(solution) = normalized_solution {
        validate_content_text(solution, &location, "solution", &mut issues);
    }
    if let Some(answer) = normalized_answer {
        validate_content_text(answer, &location, "answer", &mut issues);
    }

    issues
}

pub(crate) fn format_issues(issues: &[ContentIssue]) -> String {
    issues
        .iter()
        .take(20)
        .map(|issue| format!("{}: {}", issue.location, issue.message))
        .collect::<Vec<_>>()
        .join("; ")
}

fn validate_variant(
    course_id: &str,
    variant: &TaskVariant,
    bank_snapshot: bool,
    issues: &mut Vec<ContentIssue>,
) {
    let location = format!("variant:{}", variant.id);
    validate_required_text(&variant.content, &location, "content", issues);
    validate_content_text(&variant.content, &location, "content", issues);
    validate_required_illustration(
        &variant.content,
        !variant.attachments.0.is_empty(),
        &location,
        issues,
    );

    if !variant.answer_tolerance.is_finite() || variant.answer_tolerance < 0.0 {
        issues.push(ContentIssue::new(
            &location,
            "answer_tolerance must be a finite non-negative number",
        ));
    }

    for (field, value) in [
        ("reference_solution", variant.reference_solution.as_deref()),
        ("reference_answer", variant.reference_answer.as_deref()),
    ] {
        if let Some(value) = value {
            if value.trim().is_empty() {
                issues.push(ContentIssue::new(
                    &location,
                    format!("{field} must be null rather than blank"),
                ));
            } else {
                validate_content_text(value, &location, field, issues);
            }
        }
    }

    if reference_sign_conflicts(
        variant.reference_solution.as_deref(),
        variant.reference_answer.as_deref(),
    ) {
        issues.push(ContentIssue::new(
            &location,
            "reference_solution and reference_answer contradict each other by sign",
        ));
    }

    if bank_snapshot
        && variant.reference_solution.as_deref().map(str::trim).filter(|v| !v.is_empty()).is_none()
        && variant.reference_answer.as_deref().map(str::trim).filter(|v| !v.is_empty()).is_none()
    {
        issues.push(ContentIssue::new(
            &location,
            "task-bank assessment variants require a reviewed reference answer or solution",
        ));
    }

    let expected_segment = format!("/courses/{course_id}/materials/task-bank-image/");
    for attachment in &variant.attachments.0 {
        let attachment = attachment.trim();
        let scoped_path = attachment
            .strip_prefix("/api/v1")
            .or_else(|| attachment.strip_prefix("api/v1"))
            .unwrap_or(attachment);
        if attachment.is_empty()
            || attachment.contains(['\r', '\n'])
            || attachment.contains("..")
            || !scoped_path.starts_with('/')
            || !scoped_path.contains(&expected_segment)
        {
            issues.push(ContentIssue::new(
                &location,
                "attachments must use the protected, course-scoped task-bank image endpoint",
            ));
        }
    }
}

fn validate_required_text(
    value: &str,
    location: &str,
    field: &str,
    issues: &mut Vec<ContentIssue>,
) {
    if value.trim().is_empty() {
        issues.push(ContentIssue::new(location, format!("{field} must not be empty")));
    }
}

fn validate_content_text(value: &str, location: &str, field: &str, issues: &mut Vec<ContentIssue>) {
    // Do not mistake LaTeX commands such as \\nu, \\neq or \\nabla for escaped newlines.
    if value.match_indices("\\n").any(|(index, _)| {
        value[index + 2..].chars().next().is_none_or(|c| !c.is_ascii_alphabetic())
    }) {
        issues.push(ContentIssue::new(
            location,
            format!("{field} contains a literal escaped newline"),
        ));
    }
    if !braces_are_balanced(value) {
        issues
            .push(ContentIssue::new(location, format!("{field} contains unbalanced LaTeX braces")));
    }
    if !math_delimiters_are_balanced(value) {
        issues.push(ContentIssue::new(
            location,
            format!("{field} contains unbalanced math delimiters"),
        ));
    }
    if has_non_self_contained_reference(value) {
        issues.push(ContentIssue::new(
            location,
            format!("{field} depends on another solution, answer, or neighboring task"),
        ));
    }
}

fn validate_required_illustration(
    value: &str,
    has_images: bool,
    location: &str,
    issues: &mut Vec<ContentIssue>,
) {
    if references_illustration(value) && !has_images {
        issues.push(ContentIssue::new(
            location,
            "content references a drawing, graph, diagram, or scheme but has no image attachment",
        ));
    }
}

fn references_illustration(value: &str) -> bool {
    let normalized = value.to_lowercase();
    if normalized.contains("![") || normalized.contains("<img") {
        return true;
    }
    if let Some(after_figure) = normalized.split("рис.").nth(1) {
        if after_figure.trim_start().chars().next().is_some_and(|value| value.is_ascii_digit()) {
            return true;
        }
    }

    // A diagram the student must construct and a reaction scheme already written
    // inline are self-contained, unlike a referenced external illustration.
    let constructs_diagram =
        ["составить диаграмм", "составьте диаграмм", "построить диаграмм", "постройте диаграмм"]
            .iter()
            .any(|phrase| normalized.contains(phrase));
    let inline_scheme = normalized.contains("схем")
        && ["→", "->", "\\rightarrow", "\\text{1)", "\\text{2)"]
            .iter()
            .any(|marker| normalized.contains(marker));
    let referenced_nouns = ["рисун", "график", "диаграмм", "схем"]
        .into_iter()
        .filter(|noun| {
            !(*noun == "диаграмм" && constructs_diagram || *noun == "схем" && inline_scheme)
        })
        .collect::<Vec<_>>();
    let prepositions = ["по ", "на ", "согласно ", "из "];
    let explicitly_located = prepositions.iter().any(|preposition| {
        referenced_nouns.iter().any(|noun| normalized.contains(&format!("{preposition}{noun}")))
    });
    let described_as_provided = normalized.find("привед").is_some_and(|index| {
        referenced_nouns.iter().any(|noun| normalized[index..].contains(noun))
    });
    explicitly_located
        || described_as_provided
        || ["линии на график", "точки на график", "кривая на график"]
            .iter()
            .any(|phrase| normalized.contains(phrase))
}

fn has_non_self_contained_reference(value: &str) -> bool {
    let normalized = value.to_lowercase();
    [
        "см. решение",
        "см. ответ",
        "см. пояснение",
        "смотри решение",
        "смотрите решение",
        "предыдущая задача",
        "предыдущей задаче",
        "предыдущей задачи",
        "следующая задача",
        "следующей задаче",
        "как указано выше",
        "как указано ниже",
        "как показано выше",
        "как показано ниже",
    ]
    .iter()
    .any(|marker| normalized.contains(marker))
}

fn reference_sign_conflicts(solution: Option<&str>, answer: Option<&str>) -> bool {
    let Some((solution_value, solution_unit)) =
        solution.and_then(crate::services::grading_constraints::infer_single_quantity)
    else {
        return false;
    };
    let Some((answer_value, answer_unit)) =
        answer.and_then(crate::services::grading_constraints::infer_single_quantity)
    else {
        return false;
    };
    let Some(converted_answer) = crate::services::grading_constraints::convert_quantity(
        answer_value,
        &answer_unit,
        &solution_unit,
    ) else {
        return false;
    };
    let scale = solution_value.abs().max(converted_answer.abs());
    solution_value.signum() != converted_answer.signum()
        && scale > f64::EPSILON
        && (solution_value + converted_answer).abs() <= (scale * 1e-9).max(1e-9)
}

fn validate_formulas(formulas: &[String], location: &str, issues: &mut Vec<ContentIssue>) {
    for (index, formula) in formulas.iter().enumerate() {
        if formula.trim().is_empty() {
            issues
                .push(ContentIssue::new(location, format!("formulas[{index}] must not be empty")));
        } else {
            validate_content_text(formula, location, &format!("formulas[{index}]"), issues);
        }
    }
}

fn validate_units(units: &[Value], location: &str, issues: &mut Vec<ContentIssue>) {
    for (index, unit) in units.iter().enumerate() {
        let valid = match unit {
            Value::String(value) => !value.trim().is_empty(),
            Value::Object(object) => ["symbol", "unit", "name"]
                .iter()
                .filter_map(|key| object.get(*key).and_then(Value::as_str))
                .any(|value| !value.trim().is_empty()),
            _ => false,
        };
        if !valid {
            issues.push(ContentIssue::new(
                location,
                format!("units[{index}] must be a non-empty string or a named unit object"),
            ));
        }
    }
}

pub(crate) fn normalize_rubric(rubric: &Value, task_max_score: f64) -> Value {
    let mut result = rubric.clone();
    // Older bank snapshots contain source metadata only. They use the same
    // single answer criterion as newly imported bank tasks, without rewriting
    // historical rows or inventing criteria for teacher-authored tasks.
    if result.get("source").and_then(Value::as_str) == Some("task_bank")
        && result.get("criteria").and_then(Value::as_array).is_none_or(Vec::is_empty)
    {
        result["criteria"] = serde_json::json!([{
            "criterion_name": "Correct answer", "max_score": task_max_score,
        }]);
    }
    if let Some(criteria) = result.get_mut("criteria").and_then(Value::as_array_mut) {
        // The original exam editor stores proportions; preserve the teacher's
        // weighting while presenting explicit points to the grader.
        for criterion in criteria {
            if criterion.get("max_score").is_none() && criterion.get("maxScore").is_none() {
                if let Some(weight) = criterion.get("weight").and_then(Value::as_f64) {
                    criterion["max_score"] = serde_json::json!(weight * task_max_score);
                }
            }
        }
    }
    result
}

fn validate_rubric(
    rubric: &Value,
    task_max_score: f64,
    location: &str,
    issues: &mut Vec<ContentIssue>,
) {
    let normalized = normalize_rubric(rubric, task_max_score);
    let rubric = &normalized;
    if !rubric.is_object() {
        issues.push(ContentIssue::new(location, "rubric must be a JSON object"));
        return;
    }
    let Some(criteria) = rubric.get("criteria").and_then(Value::as_array) else {
        issues.push(ContentIssue::new(location, "rubric.criteria must be a non-empty array"));
        return;
    };
    if criteria.is_empty() {
        issues.push(ContentIssue::new(location, "rubric.criteria must contain at least one item"));
        return;
    }

    let mut total = 0.0;
    for (index, criterion) in criteria.iter().enumerate() {
        let has_name = ["criterion_name", "name", "title"]
            .iter()
            .filter_map(|key| criterion.get(*key).and_then(Value::as_str))
            .any(|name| !name.trim().is_empty());
        if !has_name {
            issues.push(ContentIssue::new(
                location,
                format!("rubric.criteria[{index}] requires a non-empty name"),
            ));
        }
        let Some(score) = criterion
            .get("max_score")
            .or_else(|| criterion.get("maxScore"))
            .and_then(Value::as_f64)
        else {
            issues.push(ContentIssue::new(
                location,
                format!("rubric.criteria[{index}].max_score is required"),
            ));
            continue;
        };
        if !score.is_finite() || score < 0.0 {
            issues.push(ContentIssue::new(
                location,
                format!("rubric.criteria[{index}].max_score must be finite and non-negative"),
            ));
            continue;
        }
        total += score;
    }
    if task_max_score.is_finite() && (total - task_max_score).abs() > SCORE_EPSILON {
        issues.push(ContentIssue::new(
            location,
            format!("rubric criteria total ({total}) must equal task max_score ({task_max_score})"),
        ));
    }
}

fn validate_validation_rules(
    rules: &Value,
    task_max_score: f64,
    location: &str,
    issues: &mut Vec<ContentIssue>,
) {
    if !rules.is_object() {
        issues.push(ContentIssue::new(location, "validation_rules must be a JSON object"));
        return;
    }

    let Some(numeric) = rules.get("numeric_answer") else {
        return;
    };
    let Some(numeric) = numeric.as_object() else {
        issues.push(ContentIssue::new(location, "numeric_answer must be a JSON object"));
        return;
    };
    let expected = numeric.get("value").and_then(Value::as_f64);
    if !expected.is_some_and(f64::is_finite) {
        issues.push(ContentIssue::new(location, "numeric_answer.value must be a finite number"));
    }
    if numeric.get("unit").and_then(Value::as_str).map(str::trim).is_none_or(str::is_empty) {
        issues.push(ContentIssue::new(location, "numeric_answer.unit must be a non-empty string"));
    }
    for key in ["absolute_tolerance", "relative_tolerance"] {
        if let Some(value) = numeric.get(key) {
            if !value.as_f64().is_some_and(|value| value.is_finite() && value >= 0.0) {
                issues.push(ContentIssue::new(
                    location,
                    format!("numeric_answer.{key} must be finite and non-negative"),
                ));
            }
        }
    }
    if let Some(value) = numeric.get("max_score_on_mismatch") {
        if !value
            .as_f64()
            .is_some_and(|value| value.is_finite() && value >= 0.0 && value <= task_max_score)
        {
            issues.push(ContentIssue::new(
                location,
                "numeric_answer.max_score_on_mismatch must be finite and within task max_score",
            ));
        }
    }
}

fn braces_are_balanced(value: &str) -> bool {
    let mut depth = 0_i32;
    let mut escaped = false;
    for character in value.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' {
            escaped = true;
        } else if character == '{' {
            depth += 1;
        } else if character == '}' {
            depth -= 1;
            if depth < 0 {
                return false;
            }
        }
    }
    depth == 0
}

fn math_delimiters_are_balanced(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut index = 0;
    let mut delimiters = 0_u32;
    while index < bytes.len() {
        if bytes[index] == b'\\' {
            index += 2;
            continue;
        }
        if bytes[index] == b'$' {
            delimiters += 1;
            if bytes.get(index + 1) == Some(&b'$') {
                index += 1;
            }
        }
        index += 1;
    }
    delimiters % 2 == 0
}

#[cfg(test)]
mod tests {
    use super::{
        has_non_self_contained_reference, reference_sign_conflicts, validate_bank_item,
        validate_rubric, validate_validation_rules, validate_variant,
    };
    use serde_json::json;
    use sqlx::types::Json;
    use time::{Date, Month, PrimitiveDateTime, Time};

    use crate::db::models::TaskVariant;

    #[test]
    fn bank_item_requires_a_real_answer() {
        let issues = validate_bank_item("7.1", "Условие", Some("  "), None, true, false);
        assert!(issues.iter().any(|issue| issue.message.contains("non-empty reference answer")));
    }

    #[test]
    fn latex_commands_starting_with_n_are_not_escaped_newlines() {
        for content in [r"Найдите $\nu$", r"Докажите $a \neq b$", r"Вычислите $\nabla f$"]
        {
            let issues = validate_bank_item("latex", content, Some("1"), None, true, false);
            assert!(issues.is_empty(), "{issues:?}");
        }
    }

    #[test]
    fn broken_formula_is_rejected() {
        let issues = validate_bank_item("7.2", "Вычислите $x_{1$", Some("1 K"), None, true, false);
        assert!(issues.iter().any(|issue| issue.message.contains("unbalanced LaTeX braces")));
    }

    #[test]
    fn numeric_rule_requires_value_unit_and_valid_tolerances() {
        let mut issues = Vec::new();
        validate_validation_rules(
            &json!({"numeric_answer": {"value": 780.0, "unit": "K", "relative_tolerance": -1.0}}),
            10.0,
            "task",
            &mut issues,
        );
        assert_eq!(issues.len(), 1);
        assert!(issues[0].message.contains("relative_tolerance"));
    }

    #[test]
    fn bank_item_referencing_a_figure_requires_an_image() {
        let issues = validate_bank_item(
            "7.3",
            "По графику определите температуру",
            Some("300 K"),
            None,
            true,
            false,
        );
        assert!(issues.iter().any(|issue| issue.message.contains("no image attachment")));
    }

    #[test]
    fn instructions_to_create_a_graph_or_scheme_do_not_require_an_image() {
        for text in ["Постройте график зависимости", "Составьте схему реакции"]
        {
            let issues = validate_bank_item("7.4", text, Some("готово"), None, true, false);
            assert!(
                !issues.iter().any(|issue| issue.message.contains("no image attachment")),
                "false image requirement for {text}: {issues:?}"
            );
        }
    }

    #[test]
    fn exam_variant_referencing_a_figure_requires_an_attachment() {
        let variant = TaskVariant {
            id: "variant-with-missing-figure".to_string(),
            course_id: "course".to_string(),
            task_type_id: "task".to_string(),
            content: "По рисунку 1 найдите ответ".to_string(),
            parameters: Json(json!({})),
            reference_solution: Some("Ответ вычисляется напрямую".to_string()),
            reference_answer: Some("2 mol".to_string()),
            answer_tolerance: 0.01,
            attachments: Json(vec![]),
            created_at: PrimitiveDateTime::new(
                Date::from_calendar_date(2026, Month::January, 1).unwrap(),
                Time::MIDNIGHT,
            ),
        };
        let mut issues = Vec::new();
        validate_variant("course", &variant, false, &mut issues);
        assert!(issues.iter().any(|issue| issue.message.contains("no image attachment")));
    }

    #[test]
    fn neighboring_answer_references_are_rejected_but_laws_are_not() {
        assert!(has_non_self_contained_reference("См. решение к № 12"));
        assert!(has_non_self_contained_reference("Как указано выше, подставьте значение"));
        assert!(!has_non_self_contained_reference(
            "Используйте закон Бойля — Мариотта и формулу из параграфа 3"
        ));
    }

    #[test]
    fn unique_reference_values_with_opposite_signs_are_rejected() {
        assert!(reference_sign_conflicts(Some("Получаем -12 kJ"), Some("12 kJ")));
        assert!(!reference_sign_conflicts(Some("Промежуточно 6 kJ, итог 12 kJ"), Some("-12 kJ")));
    }

    #[test]
    fn legacy_bank_snapshot_uses_the_same_rubric_as_new_bank_tasks() {
        let rubric = super::normalize_rubric(&json!({"source": "task_bank", "number": "1.1"}), 1.0);
        assert_eq!(rubric["criteria"][0]["max_score"], 1.0);
        let mut issues = Vec::new();
        validate_rubric(&rubric, 1.0, "legacy-bank", &mut issues);
        assert!(issues.is_empty());
    }

    #[test]
    fn editor_weighted_rubric_retains_teacher_proportions() {
        let mut issues = Vec::new();
        validate_rubric(
            &json!({"criteria": [
                {"name": "Method", "weight": 0.7}, {"name": "Answer", "weight": 0.3}
            ]}),
            10.0,
            "weighted",
            &mut issues,
        );
        assert!(issues.is_empty(), "{issues:?}");
    }

    #[test]
    fn empty_rubric_is_not_publishable() {
        let mut issues = Vec::new();
        validate_rubric(&json!({"criteria": []}), 5.0, "task", &mut issues);
        assert!(issues.iter().any(|issue| issue.message.contains("at least one")));
    }
}

#[cfg(test)]
mod bank_illustration_tests {
    use super::references_illustration;
    #[test]
    fn inline_reactions_and_student_constructed_diagrams_need_no_image() {
        assert!(!references_illustration("Составить диаграмму. Вычислить по диаграмме энтальпию."));
        assert!(!references_illustration(r"Процесс протекает по схеме $AB \rightarrow A+B$"));
        assert!(!references_illustration(
            r"Какая из схем отражает процесс? $\text{1) HCl}=H^{+}+Cl^{-}$"
        ));
        assert!(references_illustration("По схеме на рисунке определите результат."));
        assert!(references_illustration("Составить диаграмму по рисунку 1."));
    }
}
