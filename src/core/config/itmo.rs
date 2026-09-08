#[derive(Clone)]
pub(crate) struct ItmoConfig {
    pub enabled: bool,
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
    pub enrollment_rules: Vec<EnrollmentRule>,
}
impl ItmoConfig {
    pub(crate) fn load() -> Result<Self, crate::core::config::ConfigError> {
        use crate::core::config::ConfigError;
        let var = |k| std::env::var(k).unwrap_or_default();
        let mut c = Self {
            enabled: var("ITMO_ID_ENABLED") == "true",
            client_id: var("ITMO_ID_CLIENT_ID"),
            client_secret: var("ITMO_ID_CLIENT_SECRET"),
            redirect_uri: var("ITMO_ID_REDIRECT_URI"),
            enrollment_rules: vec![EnrollmentRule {
                course_slug: "infochem-29".into(),
                study_year: 2,
                teacher_isu: vec![],
                groups: var("ITMO_ID_ALLOWED_GROUPS")
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect(),
            }],
        };
        let rules = var("ITMO_ID_ENROLLMENT_RULES");
        if !rules.trim().is_empty() {
            c.enrollment_rules =
                serde_json::from_str(&rules).map_err(|_| ConfigError::InvalidValue {
                    field: "ITMO_ID_ENROLLMENT_RULES",
                    value: "Expected an array of course_slug, study_year and groups rules".into(),
                })?;
            if c.enrollment_rules.iter().any(|r| {
                r.course_slug.trim().is_empty()
                    || !(1..=8).contains(&r.study_year)
                    || r.groups.iter().any(|g| g.trim().is_empty())
                    || r.teacher_isu.iter().any(|isu| *isu <= 0)
            }) {
                return Err(ConfigError::InvalidValue{field:"ITMO_ID_ENROLLMENT_RULES",value:"Each rule needs a course slug, study year 1–8 and exact nonempty group names".into()});
            }
        }
        if c.enabled && (c.client_id.is_empty() || c.client_secret.is_empty()) {
            return Err(ConfigError::MissingSecret("ITMO_ID_CLIENT_ID/ITMO_ID_CLIENT_SECRET"));
        }
        if c.enabled
            && !matches!(
                c.redirect_uri.as_str(),
                "https://picrete.ru/auth/itmo/callback"
                    | "https://picrete.com/auth/itmo/callback"
                    | "http://localhost:8080/auth/itmo/callback"
            )
        {
            return Err(ConfigError::InvalidValue {
                field: "ITMO_ID_REDIRECT_URI",
                value: "Use a registered Picrete callback URL".into(),
            });
        }
        Ok(c)
    }
}

impl std::fmt::Debug for ItmoConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ItmoConfig")
            .field("enabled", &self.enabled)
            .field("redirect_uri", &self.redirect_uri)
            .field("enrollment_rule_count", &self.enrollment_rules.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EnrollmentRule {
    pub course_slug: String,
    pub study_year: i64,
    pub groups: Vec<String>,
    #[serde(default)]
    pub teacher_isu: Vec<i64>,
}
