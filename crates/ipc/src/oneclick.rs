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

pub fn from_args() -> Option<OneClick> {
    let prefix = format!("{SCHEME}:");
    std::env::args().find_map(|arg| arg.starts_with(&prefix).then(|| parse(&arg)).flatten())
}
