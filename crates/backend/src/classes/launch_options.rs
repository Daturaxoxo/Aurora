#[derive(Debug, Default, PartialEq, Eq)]
pub struct LaunchOptions {
    pub env: Vec<(String, String)>,
    pub wrapper: Vec<String>,
    pub trailing_args: Vec<String>,
    pub has_command: bool,
}

const COMMAND_PLACEHOLDER: &str = "%command%";

pub fn parse(input: &str) -> LaunchOptions {
    let mut opts = LaunchOptions::default();

    let tokens = split(input.trim());
    let mut tokens = tokens.into_iter().peekable();

    while let Some(token) = tokens.peek() {
        let Some((key, value)) = env_assignment(token) else {
            break;
        };
        opts.env.push((key, value));
        tokens.next();
    }

    for token in tokens.by_ref() {
        if token == COMMAND_PLACEHOLDER {
            opts.has_command = true;
            break;
        }
        opts.wrapper.push(token);
    }

    opts.trailing_args.extend(tokens);

    if !opts.has_command {
        opts.trailing_args = std::mem::take(&mut opts.wrapper);
    }

    opts
}

fn env_assignment(token: &str) -> Option<(String, String)> {
    let (key, value) = token.split_once('=')?;

    if key.is_empty() || key.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    if !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }

    Some((key.to_string(), value.to_string()))
}

fn split(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_token = false;
    let mut quote: Option<char> = None;

    for ch in input.chars() {
        match quote {
            Some(q) if ch == q => quote = None,
            Some(_) => current.push(ch),
            None if ch == '"' || ch == '\'' => {
                quote = Some(ch);
                in_token = true;
            }
            None if ch.is_whitespace() => {
                if in_token {
                    tokens.push(std::mem::take(&mut current));
                    in_token = false;
                }
            }
            None => {
                current.push(ch);
                in_token = true;
            }
        }
    }
    if in_token {
        tokens.push(current);
    }

    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn empty_input_yields_nothing() {
        assert_eq!(parse(""), LaunchOptions::default());
        assert_eq!(parse("   "), LaunchOptions::default());
    }

    #[test]
    fn env_and_wrapper() {
        let opts = parse("PROTON_LOG=1 obs-gamecapture %command%");
        assert_eq!(opts.env, env(&[("PROTON_LOG", "1")]));
        assert_eq!(opts.wrapper, strings(&["obs-gamecapture"]));
        assert!(opts.trailing_args.is_empty());
        assert!(opts.has_command);
    }

    #[test]
    fn trailing_args_go_to_the_game() {
        let opts = parse("%command% -windowed -w 1280");
        assert!(opts.env.is_empty());
        assert!(opts.wrapper.is_empty());
        assert_eq!(opts.trailing_args, strings(&["-windowed", "-w", "1280"]));
        assert!(opts.has_command);
    }

    #[test]
    fn without_a_placeholder_everything_is_game_args() {
        let opts = parse("PROTON_LOG=1 -windowed -novid");
        assert_eq!(opts.env, env(&[("PROTON_LOG", "1")]));
        assert!(opts.wrapper.is_empty());
        assert_eq!(opts.trailing_args, strings(&["-windowed", "-novid"]));
        assert!(!opts.has_command);
    }

    #[test]
    fn assignments_after_the_wrapper_are_wrapper_arguments() {
        let opts = parse("gamemoderun PROTON_LOG=1 %command%");
        assert!(opts.env.is_empty());
        assert_eq!(opts.wrapper, strings(&["gamemoderun", "PROTON_LOG=1"]));
        assert!(opts.has_command);
    }

    #[test]
    fn quoted_values_keep_their_spaces() {
        let opts = parse(r#"WINEDLLOVERRIDES="version,dsound=n,b" MANGOHUD=1 %command%"#);
        assert_eq!(
            opts.env,
            env(&[
                ("WINEDLLOVERRIDES", "version,dsound=n,b"),
                ("MANGOHUD", "1"),
            ])
        );
        assert!(opts.wrapper.is_empty());
        assert!(opts.has_command);
    }

    #[test]
    fn quoted_wrapper_arguments_stay_one_token() {
        let opts = parse("strangle --arg 'a b' %command% 'c d'");
        assert_eq!(opts.wrapper, strings(&["strangle", "--arg", "a b"]));
        assert_eq!(opts.trailing_args, strings(&["c d"]));
    }

    #[test]
    fn flags_that_contain_equals_are_not_assignments() {
        let opts = parse("--set=1 %command%");
        assert!(opts.env.is_empty());
        assert_eq!(opts.wrapper, strings(&["--set=1"]));
    }

    #[test]
    fn only_the_first_placeholder_splits_the_string() {
        let opts = parse("wrapper %command% -a %command% -b");
        assert_eq!(opts.wrapper, strings(&["wrapper"]));
        assert_eq!(opts.trailing_args, strings(&["-a", "%command%", "-b"]));
    }

    #[test]
    fn only_env_works() {
        let opts = parse("MANGOHUD=1");
        assert_eq!(opts.env, env(&[("MANGOHUD", "1")]));
        assert!(opts.wrapper.is_empty());
        assert!(!opts.has_command);
    }
}
