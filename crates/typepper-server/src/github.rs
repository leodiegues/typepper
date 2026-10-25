//! What the server asks GitHub: a user's public ssh keys (to check a key
//! linking to them is theirs), their id (logins can be renamed), and, for web
//! login, who signed in. `TYPEPPER_GITHUB` points all of it at a stand-in,
//! for tests.

use std::time::Duration;

/// A GitHub login: 1-39 letters, digits or inner single hyphens.
pub fn valid_login(s: &str) -> bool {
    (1..=39).contains(&s.len())
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        && !s.starts_with('-')
        && !s.ends_with('-')
        && !s.contains("--")
}

/// `login`'s public keys as `algorithm base64` pairs.
pub async fn keys(login: &str) -> Result<Vec<String>, String> {
    let url = format!("{}/{login}.keys", web());
    let text = get(url, None).await?;
    Ok(text
        .lines()
        .filter_map(|l| {
            let mut w = l.split_whitespace();
            Some(format!("{} {}", w.next()?, w.next()?))
        })
        .collect())
}

/// `login`'s numeric id and its login as GitHub spells it.
pub async fn user(login: &str) -> Result<(i64, String), String> {
    let url = format!("{}/users/{login}", api());
    id_and_login(&get(url, None).await?)
}

/// Web login: who signed in at GitHub, from the `code` it sent them back
/// with (to `callback`, when it isn't the app's first callback URL). The
/// token is used once, here, and never kept.
pub async fn signed_in(
    client_id: &str,
    secret: &str,
    code: &str,
    callback: Option<&str>,
) -> Result<(i64, String), String> {
    let url = format!("{}/login/oauth/access_token", web());
    let mut form = vec![
        ("client_id", client_id.to_string()),
        ("client_secret", secret.to_string()),
        ("code", code.to_string()),
    ];
    form.extend(callback.map(|c| ("redirect_uri", c.to_string())));
    let answer: serde_json::Value =
        serde_json::from_str(&call(url, None, Some(form)).await?).map_err(|e| e.to_string())?;
    // a bad or used code still answers 200, with an `error`
    let token = answer["access_token"].as_str().ok_or(format!(
        "github: {}",
        answer["error"].as_str().unwrap_or("no token")
    ))?;
    id_and_login(&get(format!("{}/user", api()), Some(token.to_string())).await?)
}

fn id_and_login(json: &str) -> Result<(i64, String), String> {
    let user: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let id = user["id"].as_i64().ok_or("no id")?;
    let login = user["login"].as_str().ok_or("no login")?.to_string();
    Ok((id, login))
}

fn web() -> String {
    std::env::var("TYPEPPER_GITHUB").unwrap_or("https://github.com".into())
}

fn api() -> String {
    std::env::var("TYPEPPER_GITHUB").unwrap_or("https://api.github.com".into())
}

async fn get(url: String, token: Option<String>) -> Result<String, String> {
    call(url, token, None).await
}

/// A GET, or a form POST when there's a `form`; JSON asked for either way.
async fn call(
    url: String,
    token: Option<String>,
    form: Option<Vec<(&'static str, String)>>,
) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(10)))
            .build()
            .into();
        // GitHub's API turns away requests without a User-Agent
        let auth = token.map(|t| format!("Bearer {t}"));
        let response = match form {
            None => {
                let mut req = agent.get(&url).header("User-Agent", "typepper-server");
                if let Some(auth) = &auth {
                    req = req.header("Authorization", auth);
                }
                req.call()
            }
            Some(form) => agent
                .post(&url)
                .header("User-Agent", "typepper-server")
                .header("Accept", "application/json")
                .send_form(form.iter().map(|(k, v)| (*k, v.as_str()))),
        };
        response
            .map_err(|e| format!("github: {e}"))?
            .body_mut()
            .read_to_string()
            .map_err(|e| format!("github: {e}"))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logins_are_what_github_allows() {
        for ok in [
            "leodiegues",
            "a",
            "y0sif",
            "Vaishnav-Sabari-Girish",
            &"a".repeat(39),
        ] {
            assert!(valid_login(ok), "{ok}");
        }
        for bad in [
            "",
            "-a",
            "a-",
            "a--b",
            "a b",
            "a/b",
            "../x",
            "é",
            &"a".repeat(40),
        ] {
            assert!(!valid_login(bad), "{bad:?}");
        }
    }
}
