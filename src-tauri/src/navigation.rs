use tauri::Url;

pub const CASESIM_URL: &str = "https://shaedy.de/casesim/";

pub fn account_url(current: Option<&Url>) -> Url {
    let return_url = current
        .filter(|url| {
            url.host_str() == Some("shaedy.de") && classify(url) == Destination::Application
        })
        .map(Url::as_str)
        .unwrap_or(CASESIM_URL);
    let mut account: Url = "https://auth.shaedy.de/".parse().unwrap();
    account.query_pairs_mut().append_pair("return", return_url);
    account
}

#[derive(Debug, PartialEq, Eq)]
pub enum Destination {
    Application,
    Browser,
    Block,
}

pub fn is_recovery(url: &Url) -> bool {
    is_local_document(url, &["/", "/index.html"])
}

pub fn is_control(url: &Url) -> bool {
    is_local_document(url, &["/settings.html"])
}

fn is_local_document(url: &Url, paths: &[&str]) -> bool {
    ((url.scheme() == "tauri" && url.host_str() == Some("localhost"))
        || (matches!(url.scheme(), "http" | "https") && url.host_str() == Some("tauri.localhost")))
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && paths.contains(&url.path())
}

pub fn classify(url: &Url) -> Destination {
    if !url.username().is_empty() || url.password().is_some() {
        return Destination::Block;
    }
    if !matches!(url.scheme(), "https" | "http") {
        return Destination::Block;
    }
    let path = url.path();
    let lower = path.to_ascii_lowercase();
    let safe_path = !["%2f", "%5c", "%00"]
        .iter()
        .any(|part| lower.contains(part));
    let trusted = url.scheme() == "https" && url.port_or_known_default() == Some(443);
    let casesim = url.host_str() == Some("shaedy.de")
        && (path == "/casesim" || path.starts_with("/casesim/"));
    let auth = url.host_str() == Some("auth.shaedy.de")
        && (matches!(path, "/" | "/login" | "/privacy" | "/privacy.html")
            || path.starts_with("/auth/"));
    if trusted && safe_path && (casesim || auth) {
        Destination::Application
    } else if matches!(
        url.host_str(),
        Some("localhost" | "tauri.localhost" | "ipc.localhost")
    ) {
        Destination::Block
    } else {
        Destination::Browser
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_navigation_preserves_only_a_trusted_game_return() {
        let deep_link = Url::parse("https://shaedy.de/casesim/inventory?item=42#wear").unwrap();
        let account = account_url(Some(&deep_link));
        assert_eq!(classify(&account), Destination::Application);
        assert_eq!(
            account
                .query_pairs()
                .find(|(key, _)| key == "return")
                .unwrap()
                .1,
            deep_link.as_str()
        );
        for candidate in [
            "https://evil.test/casesim/",
            "https://auth.shaedy.de/?reset=private",
            "http://tauri.localhost/index.html",
            "https://shaedy.de/admin",
            "https://user:secret@shaedy.de/casesim/",
        ] {
            let account = account_url(Some(&Url::parse(candidate).unwrap()));
            assert_eq!(
                account
                    .query_pairs()
                    .find(|(key, _)| key == "return")
                    .unwrap()
                    .1,
                CASESIM_URL
            );
        }
    }

    #[test]
    fn auth_and_deep_links_stay_in_application() {
        for url in [
            CASESIM_URL,
            "https://shaedy.de/casesim",
            "https://shaedy.de/casesim/inventory?item=42#wear",
            "https://auth.shaedy.de/login?return=https%3A%2F%2Fshaedy.de%2Fcasesim%2Finventory",
            "https://auth.shaedy.de/",
            "https://auth.shaedy.de/?reset=opaque",
            "https://auth.shaedy.de/auth/password/change",
            "https://auth.shaedy.de/privacy.html",
        ] {
            assert_eq!(
                classify(&Url::parse(url).unwrap()),
                Destination::Application,
                "{url}"
            );
        }
    }

    #[test]
    fn unrelated_origins_and_path_confusion_cannot_replace_game() {
        for url in [
            "https://shaedy.de/",
            "https://tickets.shaedy.de/",
            "https://3d.cstrike.app/view",
            "https://shaedy.de/casesim-evil/",
            "https://shaedy.de/casesim/../admin",
            "https://shaedy.de/casesim/%2Fadmin",
            "https://shaedy.de:444/casesim/",
            "http://shaedy.de/casesim/",
            "https://auth.shaedy.de.evil.test/login",
            "https://steamcommunity.com/",
        ] {
            assert_eq!(
                classify(&Url::parse(url).unwrap()),
                Destination::Browser,
                "{url}"
            );
        }
    }

    #[test]
    fn executable_and_credential_urls_are_blocked() {
        for url in [
            "javascript:alert(1)",
            "data:text/html,test",
            "file:///C:/Windows/system.ini",
            "ms-settings:test",
            "https://user:secret@shaedy.de/casesim/",
            "http://ipc.localhost/",
            "http://localhost/private",
        ] {
            assert_eq!(
                classify(&Url::parse(url).unwrap()),
                Destination::Block,
                "{url}"
            );
        }
    }

    #[test]
    fn recovery_has_an_exact_local_boundary() {
        for url in [
            "tauri://localhost/index.html",
            "http://tauri.localhost/",
            "http://tauri.localhost/index.html?failed=1",
        ] {
            assert!(is_recovery(&Url::parse(url).unwrap()));
        }
        for url in [
            CASESIM_URL,
            "https://auth.shaedy.de/",
            "http://localhost/",
            "http://tauri.localhost.evil.test/",
            "http://tauri.localhost:8080/",
            "http://tauri.localhost/private",
            "https://user@tauri.localhost/",
        ] {
            assert!(!is_recovery(&Url::parse(url).unwrap()));
        }
    }

    #[test]
    fn controls_are_a_separate_local_document() {
        assert!(is_control(
            &Url::parse("http://tauri.localhost/settings.html").unwrap()
        ));
        for url in [
            CASESIM_URL,
            "https://auth.shaedy.de/settings.html",
            "http://tauri.localhost/index.html",
            "https://tauri.localhost.evil.test/settings.html",
            "http://tauri.localhost:8080/settings.html",
        ] {
            assert!(!is_control(&Url::parse(url).unwrap()));
        }
    }
}
