use serde_json::{Value, json};

pub fn generate(cmd: &clap::Command) -> Value {
    let commands: Vec<Value> = cmd
        .get_subcommands()
        .map(|sub| command_to_json(sub, cmd.get_name()))
        .collect();

    json!({
        "tool": cmd.get_name(),
        "version": env!("CARGO_PKG_VERSION"),
        "description": "Multi-target release orchestrator",
        "commands": commands,
    })
}

fn command_to_json(cmd: &clap::Command, parent: &str) -> Value {
    let path = format!("{parent} {}", cmd.get_name());

    let args: Vec<Value> = cmd
        .get_arguments()
        .filter(|a| a.get_id().as_str() != "help" && a.get_id().as_str() != "version")
        .map(arg_to_json)
        .collect();

    let subcommands: Vec<Value> = cmd
        .get_subcommands()
        .map(|sub| command_to_json(sub, &path))
        .collect();

    let mut obj = json!({
        "name": cmd.get_name(),
        "path": path,
    });

    if let Some(about) = cmd.get_about() {
        obj["description"] = json!(about.to_string());
    }
    if !args.is_empty() {
        obj["arguments"] = json!(args);
    }
    if !subcommands.is_empty() {
        obj["subcommands"] = json!(subcommands);
    }
    obj
}

fn arg_to_json(arg: &clap::Arg) -> Value {
    let mut obj = serde_json::Map::new();
    let name = if arg.is_positional() {
        arg.get_id().as_str().to_string()
    } else {
        arg.get_long()
            .map(|l| format!("--{l}"))
            .unwrap_or_else(|| arg.get_id().as_str().to_string())
    };
    obj.insert("name".into(), json!(name));
    if let Some(help) = arg.get_help() {
        obj.insert("description".into(), json!(help.to_string()));
    }
    let is_bool = !arg.get_action().takes_values();
    obj.insert(
        "type".into(),
        json!(if is_bool { "bool" } else { "string" }),
    );
    Value::Object(obj)
}
