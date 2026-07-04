use crate::CliArgs;

pub fn parse_cli_args() -> Result<CliArgs, Box<dyn std::error::Error>> {
    parse_cli_args_from(std::env::args().skip(1))
}

fn parse_cli_args_from(
    args: impl IntoIterator<Item = String>,
) -> Result<CliArgs, Box<dyn std::error::Error>> {
    // What：解析调用方传入的 CLI 参数序列。
    // Why：入口只支持 profile + 单个命令，先用最小解析避免把 CLI 合同扩成第二套配置系统。
    let mut command = None;
    let mut profile = None;
    let mut args_json = None;
    let mut auth_token = None;
    let mut args = args.into_iter();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "server" | "init" | "dbsetup" => {
                if command.is_some() {
                    return Err(format!("顶层命令不能重复: {arg}").into());
                }
                command = Some(arg);
            }
            "--profile" => profile = args.next(),
            "--args" => {
                if args_json.is_some() {
                    return Err("--args 不能重复使用".into());
                }
                args_json = args.next();
            }
            "--auth" => auth_token = args.next(),
            _ => return Err(format!("未知参数: {arg}").into()),
        }
    }

    if command.as_deref() == Some("dbsetup") && args_json.is_some() {
        return Err("dbsetup 不支持 --args".into());
    }
    if command.as_deref() == Some("init") && args_json.is_some() {
        return Err("init 不支持 --args".into());
    }
    if command.as_deref() == Some("server") && args_json.is_some() {
        return Err("server 不支持 --args".into());
    }

    Ok(CliArgs {
        command,
        profile,
        args_json,
        auth_token,
    })
}

pub fn parse_args_json(args_json: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    // Why：先把 CLI 输入收敛成一个 JSON object，后面的工具分发才不会接收多种入口形状。
    let request_args = serde_json::from_str::<serde_json::Value>(args_json)?;

    if !request_args.is_object() {
        return Err("--args 必须是 JSON object".into());
    }

    Ok(request_args)
}

#[cfg(test)]
mod tests {
    use super::parse_cli_args_from;

    #[test]
    fn parse_cli_args_accepts_dbsetup_command() {
        let args = vec![
            "--profile".to_string(),
            "maccodex".to_string(),
            "dbsetup".to_string(),
        ];

        let cli_args = parse_cli_args_from(args).unwrap();

        assert_eq!(cli_args.command.as_deref(), Some("dbsetup"));
        assert_eq!(cli_args.profile.as_deref(), Some("maccodex"));
    }

    #[test]
    fn parse_cli_args_rejects_unknown_command() {
        let error = match parse_cli_args_from(vec!["migrate".to_string()]) {
            Ok(_) => panic!("unknown command should be rejected"),
            Err(error) => error,
        };

        assert_eq!(error.to_string(), "未知参数: migrate");
    }

    #[test]
    fn parse_cli_args_rejects_args_for_dbsetup_command() {
        let args = vec![
            "--profile".to_string(),
            "maccodex".to_string(),
            "dbsetup".to_string(),
            "--args".to_string(),
            "{}".to_string(),
        ];
        let error = match parse_cli_args_from(args) {
            Ok(_) => panic!("dbsetup should reject --args"),
            Err(error) => error,
        };

        assert_eq!(error.to_string(), "dbsetup 不支持 --args");
    }

    #[test]
    fn parse_cli_args_rejects_duplicate_command() {
        let error = match parse_cli_args_from(vec!["init".to_string(), "dbsetup".to_string()]) {
            Ok(_) => panic!("duplicate command should be rejected"),
            Err(error) => error,
        };

        assert_eq!(error.to_string(), "顶层命令不能重复: dbsetup");
    }

    #[test]
    fn parse_cli_args_rejects_args_for_init_command() {
        let args = vec![
            "--profile".to_string(),
            "maccodex".to_string(),
            "init".to_string(),
            "--args".to_string(),
            "{}".to_string(),
        ];
        let error = match parse_cli_args_from(args) {
            Ok(_) => panic!("init should reject --args"),
            Err(error) => error,
        };

        assert_eq!(error.to_string(), "init 不支持 --args");
    }

    #[test]
    fn parse_cli_args_rejects_args_for_server_command() {
        let args = vec![
            "server".to_string(),
            "--args".to_string(),
            "{}".to_string(),
        ];
        let error = match parse_cli_args_from(args) {
            Ok(_) => panic!("server should reject --args"),
            Err(error) => error,
        };

        assert_eq!(error.to_string(), "server 不支持 --args");
    }
}
