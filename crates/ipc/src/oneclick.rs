//! The `aurora-launcher:` URI scheme used by the `GameBanana` 1-click button.
//!
//! This lives in `ipc` rather than `shared` so that `oneclick.exe` — which does
//! nothing but parse a URI and hand it to a running Aurora — can stay a few
//! hundred kilobytes instead of pulling in Slint, reqwest and tokio.

pub const SCHEME: &str = "aurora-launcher";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OneClick {
    pub url: String,
    pub model: String,
    pub item_id: u32,
}

pub fn parse(arg: &str) -> Option<OneClick> {
    let rest = arg
        .strip_prefix(&format!("{SCHEME}:"))?
        .trim_start_matches('/');

    let mut parts = rest.rsplitn(3, ',');
    let item_id = parts.next()?.trim().parse().ok()?;
    let model = parts.next()?.trim().to_owned();
    let url = percent_encoding::percent_decode_str(parts.next()?)
        .decode_utf8()
        .ok()?
        .into_owned();

    if model.is_empty() || url.is_empty() {
        return None;
    }

    Some(OneClick {
        url,
        model,
        item_id,
    })
}

/// Picks the 1-click URI out of a command line, if there is one.
pub fn from_args() -> Option<OneClick> {
    let prefix = format!("{SCHEME}:");
    std::env::args().find_map(|arg| arg.starts_with(&prefix).then(|| parse(&arg)).flatten())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_raw_uri() {
        assert_eq!(
            parse("aurora-launcher:https://gamebanana.com/mmdl/1794621,Mod,708656"),
            Some(OneClick {
                url: "https://gamebanana.com/mmdl/1794621".into(),
                model: "Mod".into(),
                item_id: 708_656,
            })
        );
    }

    #[test]
    fn parses_slash_and_percent_encoded_uri() {
        assert_eq!(
            parse("aurora-launcher://https%3A%2F%2Fgamebanana.com%2Fdl%2F1794621,Mod,708656"),
            Some(OneClick {
                url: "https://gamebanana.com/dl/1794621".into(),
                model: "Mod".into(),
                item_id: 708_656,
            })
        );
    }

    #[test]
    fn splits_from_the_right() {
        assert_eq!(
            parse("aurora-launcher:https://gamebanana.com/mmdl/1?x=a,b,Mod,42"),
            Some(OneClick {
                url: "https://gamebanana.com/mmdl/1?x=a,b".into(),
                model: "Mod".into(),
                item_id: 42,
            })
        );
    }

    #[test]
    fn rejects_invalid_input() {
        for value in [
            "https://gamebanana.com/mmdl/1,Mod,2",
            "aurora-launcher:",
            "aurora-launcher:https://gamebanana.com/mmdl/1,Mod,nope",
            "aurora-launcher:https://gamebanana.com/mmdl/1,,2",
        ] {
            assert_eq!(parse(value), None, "{value}");
        }
    }
}
