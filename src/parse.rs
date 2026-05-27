use crate::CliArgs;

pub fn parse_cli_args() -> Result<CliArgs, Box<dyn std::error::Error>> {
    // Why：入口只支持 profile + 单个命令，先用最小解析避免把 CLI 合同扩成第二套配置系统。
    let mut command = None;
    let mut profile = None;
    let mut args_json = None;
    let mut admin_auth = None;
    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "server" | "init" => command = Some(arg),
            "--profile" => profile = args.next(),
            "--args" => args_json = args.next(),
            "--admin_auth" => admin_auth = args.next(),
            _ => return Err(format!("未知参数: {arg}").into()),
        }
    }

    Ok(CliArgs {
        command,
        profile,
        args_json,
        admin_auth,
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
