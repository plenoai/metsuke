use askama::Template;

#[derive(Template)]
#[template(path = "landing.html")]
pub(super) struct LandingTemplate;

#[derive(Template)]
#[template(path = "error.html")]
pub(super) struct ErrorTemplate {
    pub(super) title: String,
    pub(super) message: String,
}

#[derive(Template)]
#[template(path = "settings.html")]
pub(super) struct SettingsTemplate {
    pub(super) login: String,
    pub(super) active_page: &'static str,
    pub(super) installations: Vec<(i64, String, String)>,
    pub(super) base_url: String,
}

#[derive(Template)]
#[template(path = "repos.html")]
pub(super) struct ReposTemplate {
    pub(super) login: String,
    pub(super) active_page: &'static str,
}

#[derive(Template)]
#[template(path = "repo_detail.html")]
pub(super) struct RepoDetailTemplate {
    pub(super) login: String,
    pub(super) active_page: &'static str,
    pub(super) owner: String,
    pub(super) repo: String,
    pub(super) policy_options: Vec<&'static str>,
}

#[derive(Template)]
#[template(path = "verify_pr.html")]
pub(super) struct VerifyPrTemplate {
    pub(super) login: String,
    pub(super) active_page: &'static str,
    pub(super) owner: String,
    pub(super) repo: String,
    pub(super) policy_options: Vec<&'static str>,
}

#[derive(Template)]
#[template(path = "verify_release.html")]
pub(super) struct VerifyReleaseTemplate {
    pub(super) login: String,
    pub(super) active_page: &'static str,
    pub(super) owner: String,
    pub(super) repo: String,
    pub(super) policy_options: Vec<&'static str>,
}

#[derive(Template)]
#[template(path = "audit.html")]
pub(super) struct AuditTemplate {
    pub(super) login: String,
    pub(super) active_page: &'static str,
}
