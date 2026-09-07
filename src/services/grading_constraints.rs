use serde_json::{json, Value};

#[derive(Debug, Clone)]
pub(crate) struct ConstraintEvaluation {
    pub(crate) checks: Vec<Value>,
    pub(crate) score_cap: Option<f64>,
}

pub(crate) fn evaluate_numeric_constraints(
    constraints: &Value,
    student_text: &str,
    total_max_score: f64,
) -> ConstraintEvaluation {
    let Some(items) = constraints.get("numeric_answers").and_then(Value::as_array) else {
        return ConstraintEvaluation { checks: Vec::new(), score_cap: None };
    };

    let quantities = extract_quantities(student_text);
    let mut checks = Vec::new();
    let mut constrained_max = 0.0;
    let mut constrained_cap = 0.0;
    let mut mismatch_found = false;
    // OCR contains intermediate calculations and answers to multiple tasks.
    // Only an explicit teacher rule on a single-task, single-quantity answer
    // can safely impose a cap. Inferred matches remain advisory.
    let unambiguous = items.len() == 1
        && quantities.len() == 1
        && constraints.get("task_count").and_then(Value::as_u64) == Some(1);

    for item in items {
        let Some(expected) = item.get("value").and_then(Value::as_f64) else {
            continue;
        };
        let Some(expected_unit) = item.get("unit").and_then(Value::as_str) else {
            continue;
        };
        let max_score = item.get("max_score").and_then(Value::as_f64).unwrap_or(0.0).max(0.0);
        let absolute_tolerance =
            item.get("absolute_tolerance").and_then(Value::as_f64).unwrap_or(0.0).max(0.0);
        let relative_tolerance =
            item.get("relative_tolerance").and_then(Value::as_f64).unwrap_or(0.0).max(0.0);
        let mismatch_cap = item
            .get("max_score_on_mismatch")
            .and_then(Value::as_f64)
            .unwrap_or(0.0)
            .clamp(0.0, max_score);
        let allowed_delta = absolute_tolerance.max(expected.abs() * relative_tolerance);

        let comparable = quantities
            .iter()
            .filter_map(|quantity| {
                convert_unit(quantity.value, &quantity.unit, expected_unit)
                    .map(|value| (quantity, value))
            })
            .collect::<Vec<_>>();
        let matched =
            comparable.iter().any(|(_, value)| (*value - expected).abs() <= allowed_delta);
        let status = if matched {
            "pass"
        } else if comparable.is_empty() {
            "indeterminate"
        } else {
            mismatch_found |=
                unambiguous && item.get("enforce_score_cap").and_then(Value::as_bool) == Some(true);
            "mismatch"
        };

        constrained_max += max_score;
        constrained_cap += if status == "mismatch" { mismatch_cap } else { max_score };
        checks.push(json!({
            "task_type_id": item.get("task_type_id").cloned().unwrap_or(Value::Null),
            "status": status,
            "expected": {"value": expected, "unit": expected_unit},
            "absolute_tolerance": absolute_tolerance,
            "relative_tolerance": relative_tolerance,
            "observed": comparable.iter().map(|(quantity, converted)| json!({
                "value": quantity.value,
                "unit": quantity.unit,
                "converted_value": converted,
                "converted_unit": expected_unit,
            })).collect::<Vec<_>>(),
        }));
    }

    let score_cap = mismatch_found.then(|| {
        let unconstrained_max = (total_max_score - constrained_max).max(0.0);
        (unconstrained_max + constrained_cap).clamp(0.0, total_max_score)
    });
    ConstraintEvaluation { checks, score_cap }
}

pub(crate) fn normalize_model_score(
    score: Option<f64>,
    max_score: f64,
    cap: Option<f64>,
) -> Option<f64> {
    score.filter(|value| value.is_finite()).map(|value| {
        let bounded = value.clamp(0.0, max_score.max(0.0));
        cap.map_or(bounded, |cap| bounded.min(cap.max(0.0)))
    })
}

pub(crate) fn build_numeric_answer_constraint(
    task_type_id: &str,
    max_score: f64,
    validation_rules: &Value,
    reference_answer: Option<&str>,
    answer_tolerance: f64,
) -> Option<Value> {
    if let Some(rule) = validation_rules.get("numeric_answer").and_then(Value::as_object) {
        let value = rule.get("value").and_then(Value::as_f64).filter(|value| value.is_finite())?;
        let unit = rule
            .get("unit")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|unit| !unit.is_empty())?;
        return Some(json!({
            "task_type_id": task_type_id,
            "value": value,
            "unit": unit,
            "absolute_tolerance": rule
                .get("absolute_tolerance")
                .and_then(Value::as_f64)
                .unwrap_or(answer_tolerance)
                .max(0.0),
            "relative_tolerance": rule
                .get("relative_tolerance")
                .and_then(Value::as_f64)
                .unwrap_or(0.0)
                .max(0.0),
            "max_score": max_score,
            "max_score_on_mismatch": rule
                .get("max_score_on_mismatch")
                .and_then(Value::as_f64)
                .unwrap_or(0.0)
                .clamp(0.0, max_score.max(0.0)),
            "source": "validation_rule",
            "enforce_score_cap": rule.contains_key("max_score_on_mismatch"),
        }));
    }

    let (value, unit) = infer_single_quantity(reference_answer?)?;
    Some(json!({
        "task_type_id": task_type_id,
        "value": value,
        "unit": unit,
        "absolute_tolerance": answer_tolerance.max(0.0),
        "relative_tolerance": 0.0,
        "max_score": max_score,
        "max_score_on_mismatch": 0.0,
        "source": "reference_answer",
        "enforce_score_cap": false,
    }))
}

pub(crate) fn infer_single_quantity(text: &str) -> Option<(f64, String)> {
    let quantities = extract_quantities(text);
    if quantities.len() != 1 {
        return None;
    }
    let quantity = &quantities[0];
    if !is_supported_inferred_unit(&quantity.unit) {
        return None;
    }
    Some((quantity.value, quantity.unit.clone()))
}

pub(crate) fn convert_quantity(value: f64, observed: &str, expected: &str) -> Option<f64> {
    convert_unit(value, observed, expected)
}

#[derive(Debug)]
struct Quantity {
    value: f64,
    unit: String,
}

fn extract_quantities(text: &str) -> Vec<Quantity> {
    let characters = text.chars().collect::<Vec<_>>();
    let mut index = 0;
    let mut quantities = Vec::new();
    while index < characters.len() {
        let starts_number = characters[index].is_ascii_digit()
            || matches!(characters[index], '+' | '-' | '−')
                && characters.get(index + 1).is_some_and(char::is_ascii_digit);
        if !starts_number {
            index += 1;
            continue;
        }

        let start = index;
        index += 1;
        while index < characters.len()
            && (characters[index].is_ascii_digit() || matches!(characters[index], '.' | ','))
        {
            index += 1;
        }
        if index < characters.len() && matches!(characters[index], 'e' | 'E') {
            let exponent_marker = index;
            index += 1;
            if index < characters.len() && matches!(characters[index], '+' | '-' | '−') {
                index += 1;
            }
            let exponent_digits = index;
            while index < characters.len() && characters[index].is_ascii_digit() {
                index += 1;
            }
            if exponent_digits == index {
                index = exponent_marker;
            }
        }
        let raw_number =
            characters[start..index].iter().collect::<String>().replace('−', "-").replace(',', ".");
        let Ok(value) = raw_number.parse::<f64>() else {
            continue;
        };

        while index < characters.len() && characters[index].is_whitespace() {
            index += 1;
        }
        let unit_start = index;
        while index < characters.len()
            && (characters[index].is_alphabetic()
                || matches!(characters[index], '°' | '/' | '·' | '^')
                || matches!(characters[index], '%' | '+' | '-' | '−')
                || characters[index].is_ascii_digit())
        {
            index += 1;
        }
        if unit_start == index {
            continue;
        }
        let unit = characters[unit_start..index].iter().collect::<String>();
        quantities.push(Quantity { value, unit });
    }
    quantities
}

fn convert_unit(value: f64, observed: &str, expected: &str) -> Option<f64> {
    let observed = normalize_unit(observed);
    let expected = normalize_unit(expected);
    if observed == expected {
        return Some(value);
    }
    match (observed.as_str(), expected.as_str()) {
        ("c", "k") => Some(value + 273.15),
        ("k", "c") => Some(value - 273.15),
        _ => None,
    }
}

fn normalize_unit(unit: &str) -> String {
    unit.trim().to_lowercase().replace('°', "").replace('к', "k").replace('с', "c")
}

fn is_supported_inferred_unit(unit: &str) -> bool {
    let normalized = unit
        .trim()
        .to_lowercase()
        .replace('−', "-")
        .replace('°', "")
        .replace("мкмоль", "umol")
        .replace("ммоль", "mmol")
        .replace("моль", "mol")
        .replace("кдж", "kj")
        .replace("мдж", "mj")
        .replace("дж", "j")
        .replace("кпа", "kpa")
        .replace("мпа", "mpa")
        .replace("па", "pa")
        .replace("атм", "atm")
        .replace("мл", "ml")
        .replace('л', "l")
        .replace("кг", "kg")
        .replace("мкг", "ug")
        .replace("мг", "mg")
        .replace('г', "g")
        .replace("мин", "min")
        .replace("ч", "h")
        .replace("сек", "s")
        .replace('к', "k")
        .replace('с', "c");
    if normalized.is_empty() || normalized.len() > 40 {
        return false;
    }

    let without_powers = normalized
        .chars()
        .filter(|character| !matches!(character, '^' | '+' | '-') && !character.is_ascii_digit())
        .collect::<String>();
    let bases =
        without_powers.split(['/', '·', '*']).filter(|base| !base.is_empty()).collect::<Vec<_>>();
    !bases.is_empty()
        && bases.iter().all(|base| {
            matches!(
                *base,
                "k" | "c"
                    | "g"
                    | "kg"
                    | "mg"
                    | "ug"
                    | "mol"
                    | "mmol"
                    | "umol"
                    | "l"
                    | "ml"
                    | "pa"
                    | "kpa"
                    | "mpa"
                    | "atm"
                    | "bar"
                    | "j"
                    | "kj"
                    | "mj"
                    | "w"
                    | "kw"
                    | "v"
                    | "mv"
                    | "a"
                    | "ma"
                    | "n"
                    | "kn"
                    | "m"
                    | "cm"
                    | "mm"
                    | "nm"
                    | "s"
                    | "min"
                    | "h"
                    | "%"
                    | "ph"
            )
        })
}

#[cfg(test)]
mod tests {
    use super::{
        build_numeric_answer_constraint, evaluate_numeric_constraints, normalize_model_score,
    };
    use serde_json::json;

    fn constraints() -> serde_json::Value {
        json!({"task_count": 1, "numeric_answers": [{
            "task_type_id": "temperature",
            "value": 780.0,
            "unit": "K",
            "relative_tolerance": 0.05,
            "absolute_tolerance": 0.0,
            "max_score": 5.0,
            "max_score_on_mismatch": 0.0,
            "enforce_score_cap": true
        }]})
    }

    #[test]
    fn explicit_single_answer_rule_caps_wrong_temperature() {
        let evaluated = evaluate_numeric_constraints(&constraints(), "Ответ: 900 K", 5.0);
        assert_eq!(evaluated.checks[0]["status"], "mismatch");
        assert_eq!(evaluated.score_cap, Some(0.0));
        assert_eq!(normalize_model_score(Some(5.0), 5.0, evaluated.score_cap), Some(0.0));
    }

    #[test]
    fn ambiguous_or_multi_task_ocr_cannot_cap_the_score() {
        let evaluated =
            evaluate_numeric_constraints(&constraints(), "Исходно 900 K, затем 600 K", 5.0);
        assert_eq!(evaluated.score_cap, None);
        let mut multi = constraints();
        multi["task_count"] = json!(2);
        assert_eq!(evaluate_numeric_constraints(&multi, "900 K", 10.0).score_cap, None);
    }

    #[test]
    fn equivalent_celsius_value_passes_kelvin_rule() {
        let evaluated = evaluate_numeric_constraints(&constraints(), "Ответ: 506,85 °C", 5.0);
        assert_eq!(evaluated.checks[0]["status"], "pass");
        assert_eq!(evaluated.score_cap, None);
    }

    #[test]
    fn model_score_is_always_bounded() {
        assert_eq!(normalize_model_score(Some(1000.0), 5.0, None), Some(5.0));
        assert_eq!(normalize_model_score(Some(-1.0), 5.0, None), Some(0.0));
    }

    #[test]
    fn inferred_reference_quantity_is_advisory() {
        let constraint = build_numeric_answer_constraint(
            "temperature",
            5.0,
            &json!({}),
            Some("Ответ: 780 K"),
            0.01,
        )
        .expect("inferred constraint");
        let evaluated = evaluate_numeric_constraints(
            &json!({"numeric_answers": [constraint]}),
            "900 K / 600 °C",
            5.0,
        );
        assert_eq!(evaluated.checks[0]["status"], "mismatch");
        assert_eq!(evaluated.score_cap, None);
    }

    #[test]
    fn ambiguous_reference_does_not_infer_a_constraint() {
        assert!(build_numeric_answer_constraint(
            "ambiguous",
            5.0,
            &json!({}),
            Some("от 770 K до 790 K"),
            0.01,
        )
        .is_none());
    }

    #[test]
    fn inference_rejects_words_as_units_and_parses_scientific_notation() {
        assert!(build_numeric_answer_constraint(
            "false-positive",
            5.0,
            &json!({}),
            Some("20 определяется условием"),
            0.01,
        )
        .is_none());

        let constraint = build_numeric_answer_constraint(
            "scientific",
            5.0,
            &json!({}),
            Some("1.2e-3 кДж·моль^-1"),
            0.000_01,
        )
        .expect("scientific constraint");
        assert_eq!(constraint["value"], 0.0012);
        assert_eq!(constraint["unit"], "кДж·моль^-1");
    }
}
