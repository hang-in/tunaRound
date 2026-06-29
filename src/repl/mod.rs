// 터미널 REPL. 명령 파싱·렌더·세션 step. I/O는 main.rs.

/// REPL 한 줄 입력의 해석 결과.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Message(String),
    Save(Option<String>),
    Help,
    Quit,
    Noop,
}

/// 한 줄을 명령으로 파싱한다. `/`로 시작하면 명령, 아니면 메시지, 공백이면 Noop.
pub fn parse_command(line: &str) -> Command {
    let line = line.trim();
    if line.is_empty() {
        return Command::Noop;
    }
    if let Some(rest) = line.strip_prefix('/') {
        let mut it = rest.splitn(2, char::is_whitespace);
        let name = it.next().unwrap_or("");
        let arg = it.next().map(|s| s.trim().to_string());
        return match name {
            "quit" | "exit" | "q" => Command::Quit,
            "help" | "h" => Command::Help,
            "save" => Command::Save(arg.filter(|s| !s.is_empty())),
            _ => Command::Message(line.to_string()),
        };
    }
    Command::Message(line.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_commands() {
        assert_eq!(parse_command("/quit"), Command::Quit);
        assert_eq!(parse_command("/help"), Command::Help);
        assert_eq!(parse_command("/save notes.md"), Command::Save(Some("notes.md".into())));
        assert_eq!(parse_command("/save"), Command::Save(None));
        assert_eq!(parse_command("이 설계 어떤가요?"), Command::Message("이 설계 어떤가요?".into()));
    }

    #[test]
    fn blank_is_noop() {
        assert_eq!(parse_command("   "), Command::Noop);
    }
}
