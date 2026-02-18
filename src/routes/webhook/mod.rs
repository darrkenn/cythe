pub mod debug;
pub mod release;

#[macro_export]
macro_rules! invalid_request {
    ($status_code:expr, $warn_message:expr) => {
        warn!("{}", $warn_message);
        return ($status_code, "").into_response();
    };
}

#[macro_export]
macro_rules! build_telefy_message {
    ($repo_name:expr) => {
        format!(
            r#"
Repo: {},
Status: Successful
            "#,
            $repo_name
        )
    };
    ($repo_name:expr, $step_failed_on:expr) => {
        format!(
            r#"
Repo: {},
Status: Failed
Step failed on: {}
            "#,
            $repo_name, $step_failed_on
        )
    };
}
