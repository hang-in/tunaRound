// REPL 한 줄 입력을 명령(Command)으로 파싱한다.

/// REPL 한 줄 입력의 해석 결과.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Message(String),
    Save(Option<String>),
    Conclude(Option<String>),
    Only {
        engine: String,
        text: String,
    },
    Write {
        engine: String,
        text: String,
    },
    Debate {
        turns: usize,
        topic: String,
    },
    Search(String),
    /// 검색 디버그: 질의→토큰화→히트 bm25/유효성 표시.
    Explain(String),
    Branches,
    Checkout(u64),
    /// 발언을 superseded로 표시(선택적으로 대체 발언 id). 유효성 지정(HITL).
    Supersede {
        id: u64,
        by: Option<u64>,
    },
    /// 발언을 rejected로 표시(검색에서 제외).
    Reject(u64),
    /// 발언에 큐레이션(증류 요약 abstraction·검색 앵커 anchors)을 남긴다(둘 중 하나만도 허용).
    Annotate {
        id: u64,
        abstraction: Option<String>,
        anchors: Option<String>,
    },
    Help,
    Quit,
    Noop,
}

/// 큰따옴표를 존중해 공백 분리 토큰을 만든다(/annotate 인자 파싱용). 따옴표 안 공백은 보존한다.
/// 여는 따옴표는 값 경계로만 쓰고 토큰에는 포함하지 않는다. 빈 따옴표("")는 빈 토큰을 만든다.
fn split_quoted(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quote = false;
    let mut has_token = false;
    for c in s.chars() {
        if c == '"' {
            in_quote = !in_quote;
            has_token = true; // "" 도 토큰으로 인정.
        } else if c.is_whitespace() && !in_quote {
            if has_token {
                out.push(std::mem::take(&mut cur));
                has_token = false;
            }
        } else {
            cur.push(c);
            has_token = true;
        }
    }
    if has_token {
        out.push(cur);
    }
    out
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
            "conclude" => Command::Conclude(arg.filter(|s| !s.is_empty())),
            "search" => match arg.filter(|s| !s.is_empty()) {
                Some(q) => Command::Search(q),
                None => Command::Message(line.to_string()),
            },
            "explain" => match arg.filter(|s| !s.is_empty()) {
                Some(q) => Command::Explain(q),
                None => Command::Message(line.to_string()),
            },
            "branches" | "tree" => Command::Branches,
            "checkout" | "co" => match arg.as_deref().and_then(|a| a.trim().parse::<u64>().ok()) {
                Some(id) => Command::Checkout(id),
                None => Command::Message(line.to_string()),
            },
            "supersede" => {
                // /supersede <id> [<by_id>]
                let mut toks = arg.as_deref().unwrap_or("").split_whitespace();
                match toks.next().and_then(|t| t.parse::<u64>().ok()) {
                    Some(id) => {
                        let by = toks.next().and_then(|t| t.parse::<u64>().ok());
                        Command::Supersede { id, by }
                    }
                    None => Command::Message(line.to_string()),
                }
            }
            "reject" => match arg.as_deref().and_then(|a| a.trim().parse::<u64>().ok()) {
                Some(id) => Command::Reject(id),
                None => Command::Message(line.to_string()),
            },
            "annotate" => {
                // /annotate <id> --abstraction "요약" --anchors "k1,k2" (둘 중 하나만도 허용).
                let toks = split_quoted(arg.as_deref().unwrap_or(""));
                match toks.first().and_then(|t| t.parse::<u64>().ok()) {
                    None => Command::Message(line.to_string()),
                    Some(id) => {
                        let mut abstraction = None;
                        let mut anchors = None;
                        // 플래그 값으로 소비할 다음 토큰. 다음 토큰이 `--`로 시작하면(=다음 플래그) 값이
                        // 없는 것으로 보고 삼키지 않는다(예 `--abstraction --anchors "x"`, CodeRabbit).
                        let take_value = |toks: &[String], i: usize| -> Option<String> {
                            toks.get(i + 1)
                                .filter(|s| !s.is_empty() && !s.starts_with("--"))
                                .cloned()
                        };
                        let mut i = 1;
                        while i < toks.len() {
                            match toks[i].as_str() {
                                "--abstraction" => {
                                    let v = take_value(&toks, i);
                                    let consumed = v.is_some();
                                    abstraction = v;
                                    i += if consumed { 2 } else { 1 };
                                }
                                "--anchors" => {
                                    let v = take_value(&toks, i);
                                    let consumed = v.is_some();
                                    anchors = v;
                                    i += if consumed { 2 } else { 1 };
                                }
                                _ => i += 1,
                            }
                        }
                        // 둘 다 비면 잘못된 사용 → 일반 메시지로 폴스루(기존 명령 패턴 답습).
                        if abstraction.is_none() && anchors.is_none() {
                            Command::Message(line.to_string())
                        } else {
                            Command::Annotate {
                                id,
                                abstraction,
                                anchors,
                            }
                        }
                    }
                }
            }
            "debate" => {
                const DEFAULT_TURNS: usize = 3;
                const MAX_TURNS: usize = 10;
                match arg.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                    None => Command::Message(line.to_string()), // 주제 없음
                    Some(rest) => {
                        // 첫 토큰이 숫자면 turns, 나머지가 topic. 아니면 전체가 topic(기본 turns).
                        let mut it = rest.splitn(2, char::is_whitespace);
                        let first = it.next().unwrap_or("");
                        match first.parse::<usize>() {
                            Ok(n) => {
                                let topic =
                                    it.next().map(|s| s.trim().to_string()).unwrap_or_default();
                                if topic.is_empty() {
                                    Command::Message(line.to_string()) // 숫자만, 주제 없음
                                } else {
                                    Command::Debate {
                                        turns: n.clamp(1, MAX_TURNS),
                                        topic,
                                    }
                                }
                            }
                            Err(_) => Command::Debate {
                                turns: DEFAULT_TURNS,
                                topic: rest.to_string(),
                            },
                        }
                    }
                }
            }
            _ => Command::Message(line.to_string()),
        };
    }
    if let Some(rest) = line.strip_prefix('@') {
        let mut it = rest.splitn(2, char::is_whitespace);
        let mut engine = it.next().unwrap_or("").to_string();
        let text = it.next().map(|s| s.trim().to_string()).unwrap_or_default();
        let write = engine.ends_with('!');
        if write {
            engine.pop(); // trailing '!' 제거
        }
        if !engine.is_empty() && !text.is_empty() {
            return if write {
                Command::Write { engine, text }
            } else {
                Command::Only { engine, text }
            };
        }
        return Command::Message(line.to_string()); // "@codex"·"@codex!"만이면 일반 메시지
    }
    Command::Message(line.to_string())
}
