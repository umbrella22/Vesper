use std::{
    collections::{HashMap, HashSet},
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

const GENERATED_NOTICE: &str = "Auto-generated. Do not edit directly.";

#[derive(Debug, Deserialize, Serialize)]
struct Manifest {
    header_guard: String,
    header_includes: Vec<String>,
    c_includes: Vec<String>,
    public_declarations: Vec<Declaration>,
    private_declarations: Vec<Declaration>,
    source_items: Vec<SourceItem>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind")]
enum Declaration {
    #[serde(rename = "enum")]
    Enum {
        name: String,
        variants: Vec<EnumVariant>,
    },
    #[serde(rename = "struct")]
    Struct { name: String, fields: Vec<Field> },
    #[serde(rename = "function")]
    Function {
        return_type: String,
        name: String,
        parameters: Vec<Parameter>,
        #[serde(default)]
        storage: FunctionStorage,
    },
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum FunctionStorage {
    #[default]
    Public,
    Extern,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct EnumVariant {
    name: String,
    value: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Field {
    ty: String,
    name: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Parameter {
    ty: String,
    name: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "kind")]
enum SourceItem {
    #[serde(rename = "static_fragment")]
    StaticFragment { path: String },
    #[serde(rename = "wrapper")]
    Wrapper {
        function: String,
        ffi_symbols: Vec<String>,
        ownership: Vec<String>,
        body_fragment: String,
    },
}

struct GeneratedShim {
    header: String,
    source: String,
}

struct Cli {
    command: Command,
    manifest_path: PathBuf,
    output_dir: PathBuf,
    source_header_path: Option<PathBuf>,
    source_c_path: Option<PathBuf>,
}

enum Command {
    Generate,
    Bootstrap,
}

fn main() -> Result<()> {
    let cli = Cli::parse(env::args().skip(1))?;
    match cli.command {
        Command::Generate => {
            let manifest = read_manifest(&cli.manifest_path)?;
            let generated = generate(&manifest, manifest_base_dir(&cli.manifest_path).as_path())?;
            write_generated(&cli.output_dir, &generated)?;
        }
        Command::Bootstrap => {
            let header_path = cli
                .source_header_path
                .as_deref()
                .context("bootstrap requires --source-header")?;
            let c_path = cli
                .source_c_path
                .as_deref()
                .context("bootstrap requires --source-c")?;
            bootstrap(&cli.manifest_path, &cli.output_dir, header_path, c_path)?;
        }
    }
    Ok(())
}

impl Cli {
    fn parse(args: impl IntoIterator<Item = String>) -> Result<Self> {
        let mut command = None;
        let mut manifest_path = None;
        let mut output_dir = None;
        let mut source_header_path = None;
        let mut source_c_path = None;

        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "generate" => command = Some(Command::Generate),
                "bootstrap" => command = Some(Command::Bootstrap),
                "--manifest" => manifest_path = Some(next_path(&mut args, "--manifest")?),
                "--out-dir" => output_dir = Some(next_path(&mut args, "--out-dir")?),
                "--source-header" => {
                    source_header_path = Some(next_path(&mut args, "--source-header")?)
                }
                "--source-c" => source_c_path = Some(next_path(&mut args, "--source-c")?),
                "-h" | "--help" => {
                    print_usage();
                    std::process::exit(0);
                }
                _ => bail!("unknown argument: {arg}"),
            }
        }

        Ok(Self {
            command: command.context("missing command: generate or bootstrap")?,
            manifest_path: manifest_path.context("missing --manifest")?,
            output_dir: output_dir.context("missing --out-dir")?,
            source_header_path,
            source_c_path,
        })
    }
}

fn next_path(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<PathBuf> {
    Ok(PathBuf::from(
        args.next()
            .with_context(|| format!("{flag} requires a value"))?,
    ))
}

fn print_usage() {
    eprintln!(
        "Usage:\n  player-ios-bridge-shim-generator generate --manifest <path> --out-dir <dir>\n  player-ios-bridge-shim-generator bootstrap --manifest <path> --out-dir <fragment-dir> --source-header <path> --source-c <path>"
    );
}

fn manifest_base_dir(manifest_path: &Path) -> PathBuf {
    manifest_path
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

fn read_manifest(path: &Path) -> Result<Manifest> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read manifest: {}", path.display()))?;
    serde_json::from_str(&content)
        .with_context(|| format!("failed to parse manifest: {}", path.display()))
}

fn write_generated(output_dir: &Path, generated: &GeneratedShim) -> Result<()> {
    let include_dir = output_dir.join("include");
    fs::create_dir_all(&include_dir).with_context(|| {
        format!(
            "failed to create output include dir: {}",
            include_dir.display()
        )
    })?;
    fs::write(
        include_dir.join("VesperPlayerKitBridgeShim.h"),
        &generated.header,
    )
    .with_context(|| "failed to write generated bridge shim header")?;
    fs::write(
        output_dir.join("VesperPlayerKitBridgeShim.c"),
        &generated.source,
    )
    .with_context(|| "failed to write generated bridge shim source")?;
    Ok(())
}

fn generate(manifest: &Manifest, manifest_dir: &Path) -> Result<GeneratedShim> {
    let header = generate_header(manifest)?;
    let source = generate_source(manifest, manifest_dir)?;
    Ok(GeneratedShim { header, source })
}

fn generate_header(manifest: &Manifest) -> Result<String> {
    let mut output = String::new();
    output.push_str("/* ");
    output.push_str(GENERATED_NOTICE);
    output.push_str(" */\n");
    output.push_str("#ifndef ");
    output.push_str(&manifest.header_guard);
    output.push('\n');
    output.push_str("#define ");
    output.push_str(&manifest.header_guard);
    output.push_str("\n\n");
    push_includes(&mut output, &manifest.header_includes);
    output.push('\n');
    push_declarations(&mut output, &manifest.public_declarations)?;
    output.push_str("#endif\n");
    Ok(output)
}

fn generate_source(manifest: &Manifest, manifest_dir: &Path) -> Result<String> {
    let mut output = String::new();
    output.push_str("/* ");
    output.push_str(GENERATED_NOTICE);
    output.push_str(" */\n");
    push_includes(&mut output, &manifest.c_includes);
    output.push('\n');
    push_declarations(&mut output, &manifest.private_declarations)?;
    let public_functions = public_function_declarations(&manifest.public_declarations);
    for source_item in &manifest.source_items {
        match source_item {
            SourceItem::StaticFragment { path } => {
                let fragment_path = manifest_dir.join(path);
                let fragment_content = fs::read_to_string(&fragment_path).with_context(|| {
                    format!(
                        "failed to read C static fragment: {}",
                        fragment_path.display()
                    )
                })?;
                if !output.ends_with("\n\n") {
                    output.push('\n');
                }
                output.push_str(fragment_content.trim_end());
                output.push_str("\n");
            }
            SourceItem::Wrapper {
                function,
                ffi_symbols,
                ownership: _,
                body_fragment,
            } => {
                let declaration = public_functions.get(function.as_str()).with_context(|| {
                    format!("wrapper fragment references unknown public function: {function}")
                })?;
                let body_path = manifest_dir.join(body_fragment);
                let body = fs::read_to_string(&body_path).with_context(|| {
                    format!(
                        "failed to read wrapper body fragment: {}",
                        body_path.display()
                    )
                })?;
                for ffi_symbol in ffi_symbols {
                    let call_pattern = format!("{ffi_symbol}(");
                    if !body.contains(&call_pattern) {
                        bail!(
                            "wrapper {function} declares FFI symbol {ffi_symbol}, but its body fragment does not call it"
                        );
                    }
                }
                if !output.ends_with("\n\n") {
                    output.push('\n');
                }
                push_function_definition_signature(
                    &mut output,
                    declaration.return_type,
                    declaration.name,
                    declaration.parameters,
                );
                output.push(' ');
                output.push_str(body.trim());
                output.push_str("\n");
            }
        }
    }
    Ok(output)
}

struct FunctionDeclaration<'a> {
    return_type: &'a str,
    name: &'a str,
    parameters: &'a [Parameter],
}

fn public_function_declarations(
    declarations: &[Declaration],
) -> HashMap<&str, FunctionDeclaration<'_>> {
    declarations
        .iter()
        .filter_map(|declaration| match declaration {
            Declaration::Function {
                return_type,
                name,
                parameters,
                ..
            } => Some((
                name.as_str(),
                FunctionDeclaration {
                    return_type,
                    name,
                    parameters,
                },
            )),
            _ => None,
        })
        .collect()
}

fn push_includes(output: &mut String, includes: &[String]) {
    for include in includes {
        output.push_str("#include ");
        output.push_str(include);
        output.push('\n');
    }
}

fn push_declarations(output: &mut String, declarations: &[Declaration]) -> Result<()> {
    for declaration in declarations {
        push_declaration(output, declaration)?;
        output.push('\n');
    }
    Ok(())
}

fn push_declaration(output: &mut String, declaration: &Declaration) -> Result<()> {
    match declaration {
        Declaration::Enum { name, variants } => {
            output.push_str("typedef enum ");
            output.push_str(name);
            output.push_str(" {\n");
            for variant in variants {
                output.push_str("  ");
                output.push_str(&variant.name);
                output.push_str(" = ");
                output.push_str(&variant.value.to_string());
                output.push_str(",\n");
            }
            output.push_str("} ");
            output.push_str(name);
            output.push_str(";\n");
        }
        Declaration::Struct { name, fields } => {
            output.push_str("typedef struct ");
            output.push_str(name);
            output.push_str(" {\n");
            for field in fields {
                output.push_str("  ");
                push_typed_name(output, &field.ty, &field.name);
                output.push_str(";\n");
            }
            output.push_str("} ");
            output.push_str(name);
            output.push_str(";\n");
        }
        Declaration::Function {
            return_type,
            name,
            parameters,
            storage,
        } => {
            push_function_declaration(output, return_type, name, parameters, storage)?;
        }
    }
    Ok(())
}

fn push_function_declaration(
    output: &mut String,
    return_type: &str,
    name: &str,
    parameters: &[Parameter],
    storage: &FunctionStorage,
) -> Result<()> {
    if matches!(storage, FunctionStorage::Extern) {
        output.push_str("extern ");
    }
    if parameters.is_empty() {
        output.push_str(return_type);
        output.push(' ');
        output.push_str(name);
        output.push_str("(void);\n");
        return Ok(());
    }

    if parameters.len() == 1 {
        output.push_str(return_type);
        output.push(' ');
        output.push_str(name);
        output.push('(');
        push_typed_name(output, &parameters[0].ty, &parameters[0].name);
        output.push_str(");\n");
        return Ok(());
    }

    output.push_str(return_type);
    output.push(' ');
    output.push_str(name);
    output.push_str("(\n");
    for (index, parameter) in parameters.iter().enumerate() {
        output.push_str("    ");
        push_typed_name(output, &parameter.ty, &parameter.name);
        if index + 1 == parameters.len() {
            output.push_str(");\n");
        } else {
            output.push_str(",\n");
        }
    }
    Ok(())
}

fn push_function_definition_signature(
    output: &mut String,
    return_type: &str,
    name: &str,
    parameters: &[Parameter],
) {
    if parameters.is_empty() {
        output.push_str(return_type);
        output.push(' ');
        output.push_str(name);
        output.push_str("(void)");
        return;
    }

    if parameters.len() == 1 {
        output.push_str(return_type);
        output.push(' ');
        output.push_str(name);
        output.push('(');
        push_typed_name(output, &parameters[0].ty, &parameters[0].name);
        output.push(')');
        return;
    }

    output.push_str(return_type);
    output.push(' ');
    output.push_str(name);
    output.push_str("(\n");
    for (index, parameter) in parameters.iter().enumerate() {
        output.push_str("    ");
        push_typed_name(output, &parameter.ty, &parameter.name);
        if index + 1 == parameters.len() {
            output.push(')');
        } else {
            output.push_str(",\n");
        }
    }
}

fn push_typed_name(output: &mut String, ty: &str, name: &str) {
    if let Some(marker) = ty.find("(*)") {
        output.push_str(&ty[..marker + 2]);
        output.push_str(name);
        output.push_str(&ty[marker + 2..]);
    } else {
        output.push_str(ty);
        if !ty.ends_with('*') {
            output.push(' ');
        }
        output.push_str(name);
    }
}

fn bootstrap(
    manifest_path: &Path,
    fragment_dir: &Path,
    header_path: &Path,
    c_path: &Path,
) -> Result<()> {
    fs::create_dir_all(fragment_dir)
        .with_context(|| format!("failed to create fragment dir: {}", fragment_dir.display()))?;

    let header = fs::read_to_string(header_path)
        .with_context(|| format!("failed to read source header: {}", header_path.display()))?;
    let source = fs::read_to_string(c_path)
        .with_context(|| format!("failed to read source C file: {}", c_path.display()))?;

    let public_declarations = parse_header_declarations(&header)?;
    let (c_includes, private_declarations, body) = parse_source_sections(&source)?;
    let public_function_names = public_declarations
        .iter()
        .filter_map(|declaration| match declaration {
            Declaration::Function { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect::<HashSet<_>>();
    let source_items = split_body_fragments(&body, &public_function_names, fragment_dir)?;

    let manifest = Manifest {
        header_guard: "VESPER_PLAYER_KIT_BRIDGE_SHIM_H".to_string(),
        header_includes: vec![
            "<stdbool.h>".to_string(),
            "<stddef.h>".to_string(),
            "<stdint.h>".to_string(),
        ],
        c_includes,
        public_declarations,
        private_declarations,
        source_items,
    };
    let manifest_content = serde_json::to_string_pretty(&manifest)?;
    fs::write(manifest_path, format!("{manifest_content}\n"))
        .with_context(|| format!("failed to write manifest: {}", manifest_path.display()))?;
    Ok(())
}

fn split_body_fragments(
    body: &str,
    public_function_names: &HashSet<&str>,
    fragment_dir: &Path,
) -> Result<Vec<SourceItem>> {
    let wrapper_dir = fragment_dir.join("fragments/wrappers");
    fs::create_dir_all(&wrapper_dir).with_context(|| {
        format!(
            "failed to create wrapper fragment dir: {}",
            wrapper_dir.display()
        )
    })?;
    let static_dir = fragment_dir.join("fragments/static");
    fs::create_dir_all(&static_dir).with_context(|| {
        format!(
            "failed to create static fragment dir: {}",
            static_dir.display()
        )
    })?;

    let mut cursor = 0;
    let mut static_group = String::new();
    let mut static_group_index = 0usize;
    let mut source_items = Vec::new();
    while let Some(item_start) = next_non_whitespace(body, cursor) {
        let open_brace = find_top_level_open_brace(body, item_start)?;
        let item_end = find_matching_brace(body, open_brace)? + 1;
        let item = &body[item_start..item_end];
        let signature = body[item_start..open_brace].trim();
        let function_name = top_level_function_name(signature)?;
        if signature.starts_with("static ") {
            if !static_group.is_empty() {
                static_group.push_str("\n\n");
            }
            static_group.push_str(item.trim());
        } else if public_function_names.contains(function_name.as_str()) {
            flush_static_group(
                fragment_dir,
                &mut source_items,
                &mut static_group,
                &mut static_group_index,
            )?;
            let body_fragment = &body[open_brace..item_end];
            let fragment_name = format!("fragments/wrappers/{function_name}.c.inc");
            let fragment_path = fragment_dir.join(&fragment_name);
            fs::write(&fragment_path, format!("{}\n", body_fragment.trim())).with_context(
                || {
                    format!(
                        "failed to write wrapper fragment: {}",
                        fragment_path.display()
                    )
                },
            )?;
            source_items.push(SourceItem::Wrapper {
                function: function_name,
                ffi_symbols: sorted_player_ffi_symbols(body_fragment),
                ownership: ownership_notes(body_fragment),
                body_fragment: fragment_name,
            });
        } else {
            bail!("unknown non-static bridge function in C body: {function_name}");
        }
        cursor = item_end;
    }
    flush_static_group(
        fragment_dir,
        &mut source_items,
        &mut static_group,
        &mut static_group_index,
    )?;

    Ok(source_items)
}

fn flush_static_group(
    fragment_dir: &Path,
    source_items: &mut Vec<SourceItem>,
    static_group: &mut String,
    static_group_index: &mut usize,
) -> Result<()> {
    if static_group.trim().is_empty() {
        return Ok(());
    }
    *static_group_index += 1;
    let fragment_name = format!("fragments/static/static-helpers-{static_group_index:02}.c.inc");
    let fragment_path = fragment_dir.join(&fragment_name);
    fs::write(&fragment_path, format!("{}\n", static_group.trim())).with_context(|| {
        format!(
            "failed to write static helper fragment: {}",
            fragment_path.display()
        )
    })?;
    source_items.push(SourceItem::StaticFragment {
        path: fragment_name,
    });
    static_group.clear();
    Ok(())
}

fn sorted_player_ffi_symbols(input: &str) -> Vec<String> {
    let mut symbols = HashSet::new();
    let mut cursor = 0;
    while let Some(offset) = input[cursor..].find("player_ffi_") {
        let start = cursor + offset;
        let end = input[start..]
            .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
            .map_or(input.len(), |relative| start + relative);
        if input[end..].starts_with('(') {
            symbols.insert(input[start..end].to_string());
        }
        cursor = end;
    }
    let mut symbols = symbols.into_iter().collect::<Vec<_>>();
    symbols.sort();
    symbols
}

fn ownership_notes(input: &str) -> Vec<String> {
    let mut notes = Vec::new();
    if input.contains("player_ffi_error_free(") {
        notes.push("Frees Rust-owned error messages with player_ffi_error_free.".to_string());
    }
    if input.contains("player_ffi_preload_command_list_free(") {
        notes.push(
            "Frees Rust-owned preload command lists after copying them to runtime DTOs."
                .to_string(),
        );
    }
    if input.contains("player_ffi_playlist_active_item_free(") {
        notes.push(
            "Frees Rust-owned playlist active item strings after copying them to runtime DTOs."
                .to_string(),
        );
    }
    if input.contains("player_ffi_download_snapshot_free(") {
        notes.push(
            "Frees Rust-owned download snapshots after copying them to runtime DTOs.".to_string(),
        );
    }
    if input.contains("player_ffi_download_command_list_free(") {
        notes.push(
            "Frees Rust-owned download command lists after copying them to runtime DTOs."
                .to_string(),
        );
    }
    if input.contains("player_ffi_download_event_list_free(") {
        notes.push(
            "Frees Rust-owned download event lists after copying them to runtime DTOs.".to_string(),
        );
    }
    if input.contains("player_ffi_track_preferences_free(") {
        notes.push(
            "Frees Rust-owned resolved track preference strings after copying them to runtime DTOs."
                .to_string(),
        );
    }
    if input.contains("player_ffi_benchmark_report_string_free(") {
        notes.push(
            "Forwards benchmark report string release to the Rust FFI free function.".to_string(),
        );
    }
    if input.contains("player_ffi_dash_bridge_string_free(") {
        notes
            .push("Forwards DASH bridge string release to the Rust FFI free function.".to_string());
    }
    if input.contains("free(") {
        notes.push("Releases C-owned bridge allocations with free.".to_string());
    }
    if input.contains("calloc(") {
        notes.push(
            "Allocates copied C bridge DTO arrays with calloc and pairs them with bridge free wrappers."
                .to_string(),
        );
    }
    notes
}

fn next_non_whitespace(input: &str, cursor: usize) -> Option<usize> {
    input[cursor..]
        .char_indices()
        .find(|(_, ch)| !ch.is_whitespace())
        .map(|(offset, _)| cursor + offset)
}

fn find_top_level_open_brace(input: &str, start: usize) -> Result<usize> {
    input[start..]
        .find('{')
        .map(|offset| start + offset)
        .with_context(|| format!("could not find function body after byte {start}"))
}

fn top_level_function_name(signature: &str) -> Result<String> {
    let open = signature
        .rfind('(')
        .with_context(|| format!("function signature missing (: {signature}"))?;
    let prefix = signature[..open].trim_end();
    let name_end = prefix.len();
    let name_start = prefix[..name_end]
        .rfind(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
        .map_or(0, |index| index + 1);
    let name = &prefix[name_start..name_end];
    if name.is_empty() {
        bail!("function signature missing name: {signature}");
    }
    Ok(name.to_string())
}

fn find_matching_brace(input: &str, open_brace: usize) -> Result<usize> {
    let bytes = input.as_bytes();
    let mut index = open_brace;
    let mut depth = 0usize;
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut in_string = false;
    let mut in_char = false;
    let mut escaped = false;

    while index < bytes.len() {
        let byte = bytes[index];
        let next = bytes.get(index + 1).copied();

        if in_line_comment {
            if byte == b'\n' {
                in_line_comment = false;
            }
            index += 1;
            continue;
        }
        if in_block_comment {
            if byte == b'*' && next == Some(b'/') {
                in_block_comment = false;
                index += 2;
            } else {
                index += 1;
            }
            continue;
        }
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            index += 1;
            continue;
        }
        if in_char {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'\'' {
                in_char = false;
            }
            index += 1;
            continue;
        }

        if byte == b'/' && next == Some(b'/') {
            in_line_comment = true;
            index += 2;
            continue;
        }
        if byte == b'/' && next == Some(b'*') {
            in_block_comment = true;
            index += 2;
            continue;
        }
        if byte == b'"' {
            in_string = true;
            index += 1;
            continue;
        }
        if byte == b'\'' {
            in_char = true;
            index += 1;
            continue;
        }
        if byte == b'{' {
            depth += 1;
        } else if byte == b'}' {
            depth = depth
                .checked_sub(1)
                .with_context(|| format!("unbalanced closing brace at byte {index}"))?;
            if depth == 0 {
                return Ok(index);
            }
        }
        index += 1;
    }

    bail!("could not find matching brace after byte {open_brace}")
}

fn parse_header_declarations(header: &str) -> Result<Vec<Declaration>> {
    let mut body = header;
    if let Some(index) = body.find("#define VESPER_PLAYER_KIT_BRIDGE_SHIM_H") {
        body = &body[index + "#define VESPER_PLAYER_KIT_BRIDGE_SHIM_H".len()..];
    }
    let mut declarations = Vec::new();
    let mut cursor = 0;
    while let Some(start) = find_next_declaration_start(body, cursor) {
        if body[start..].starts_with("typedef enum ") {
            let (declaration, end) = parse_enum_declaration(body, start)?;
            declarations.push(declaration);
            cursor = end;
        } else if body[start..].starts_with("typedef struct ") {
            let (declaration, end) = parse_struct_declaration(body, start)?;
            declarations.push(declaration);
            cursor = end;
        } else {
            let (declaration, end) = parse_function_prototype(body, start)?;
            declarations.push(declaration);
            cursor = end;
        }
    }
    Ok(declarations)
}

fn parse_source_sections(source: &str) -> Result<(Vec<String>, Vec<Declaration>, String)> {
    let mut c_includes = Vec::new();
    let mut cursor = 0;
    let generated_notice_line = format!("/* {GENERATED_NOTICE} */");
    if source.starts_with(&generated_notice_line) {
        cursor = generated_notice_line.len();
        if source[cursor..].starts_with('\n') {
            cursor += 1;
        }
    }
    for line in source[cursor..].lines() {
        if line.starts_with("#include ") {
            c_includes.push(line.trim_start_matches("#include ").to_string());
            cursor += line.len() + 1;
        } else if line.trim().is_empty() {
            cursor += line.len() + 1;
        } else {
            break;
        }
    }

    let mut declarations = Vec::new();
    let mut body_start = cursor;
    loop {
        let rest = &source[body_start..];
        let trimmed = rest.trim_start();
        body_start += rest.len() - trimmed.len();
        if trimmed.starts_with("typedef enum ") {
            let (declaration, end) = parse_enum_declaration(source, body_start)?;
            declarations.push(declaration);
            body_start = end;
        } else if trimmed.starts_with("typedef struct ") {
            let (declaration, end) = parse_struct_declaration(source, body_start)?;
            declarations.push(declaration);
            body_start = end;
        } else if trimmed.starts_with("extern ") {
            let (declaration, end) = parse_extern_prototype(source, body_start)?;
            declarations.push(declaration);
            body_start = end;
        } else {
            break;
        }
    }

    Ok((c_includes, declarations, source[body_start..].to_string()))
}

fn find_next_declaration_start(input: &str, cursor: usize) -> Option<usize> {
    let candidates = [
        input[cursor..]
            .find("typedef enum ")
            .map(|offset| cursor + offset),
        input[cursor..]
            .find("typedef struct ")
            .map(|offset| cursor + offset),
        input[cursor..]
            .find("\nbool ")
            .map(|offset| cursor + offset + 1),
        input[cursor..]
            .find("\nvoid ")
            .map(|offset| cursor + offset + 1),
        input[cursor..]
            .find("\nuint64_t ")
            .map(|offset| cursor + offset + 1),
        input[cursor..]
            .find("\nchar *")
            .map(|offset| cursor + offset + 1),
    ];
    candidates.into_iter().flatten().min()
}

fn parse_enum_declaration(input: &str, start: usize) -> Result<(Declaration, usize)> {
    let end = find_typedef_declaration_end(input, start)?;
    let block = &input[start..end];
    let name = text_between(block, "typedef enum ", " {")?
        .trim()
        .to_string();
    let body = text_between(block, "{", "}")?;
    let mut variants = Vec::new();
    for line in body.lines() {
        let line = line.trim().trim_end_matches(',');
        if line.is_empty() {
            continue;
        }
        let (name, value) = line
            .split_once('=')
            .with_context(|| format!("enum variant missing value: {line}"))?;
        variants.push(EnumVariant {
            name: name.trim().to_string(),
            value: value.trim().parse()?,
        });
    }
    Ok((Declaration::Enum { name, variants }, end))
}

fn parse_struct_declaration(input: &str, start: usize) -> Result<(Declaration, usize)> {
    let end = find_typedef_declaration_end(input, start)?;
    let block = &input[start..end];
    let name = text_between(block, "typedef struct ", " {")?
        .trim()
        .to_string();
    let body = text_between(block, "{", "}")?;
    let mut fields = Vec::new();
    for line in body.lines() {
        let line = line.trim().trim_end_matches(';');
        if line.is_empty() {
            continue;
        }
        let field = parse_named_type(line)?;
        fields.push(Field {
            ty: field.ty,
            name: field.name,
        });
    }
    Ok((Declaration::Struct { name, fields }, end))
}

fn parse_extern_prototype(input: &str, start: usize) -> Result<(Declaration, usize)> {
    let end = find_declaration_end(input, start)?;
    let block = input[start..end].trim();
    let prototype = block
        .strip_prefix("extern ")
        .context("extern declaration missing extern prefix")?;
    parse_function_declaration_text(prototype, FunctionStorage::Extern)
        .map(|declaration| (declaration, end))
}

fn parse_function_prototype(input: &str, start: usize) -> Result<(Declaration, usize)> {
    let end = find_declaration_end(input, start)?;
    parse_function_declaration_text(input[start..end].trim(), FunctionStorage::Public)
        .map(|declaration| (declaration, end))
}

fn parse_function_declaration_text(
    prototype: &str,
    storage: FunctionStorage,
) -> Result<Declaration> {
    let prototype = prototype.trim_end_matches(';').trim();
    let open = prototype
        .find('(')
        .with_context(|| format!("function declaration missing (: {prototype}"))?;
    let close = prototype
        .rfind(')')
        .with_context(|| format!("function declaration missing ): {prototype}"))?;
    let head = prototype[..open].trim();
    let (return_type, name) = split_type_and_name(head)?;
    let params_text = prototype[open + 1..close].trim();
    let parameters = if params_text.is_empty() || params_text == "void" {
        Vec::new()
    } else {
        params_text
            .split(',')
            .map(|param| parse_named_type(param.trim()))
            .collect::<Result<Vec<_>>>()?
    };
    Ok(Declaration::Function {
        return_type,
        name,
        parameters,
        storage,
    })
}

fn parse_named_type(value: &str) -> Result<Parameter> {
    let (ty, name) = split_type_and_name(value)?;
    Ok(Parameter { ty, name })
}

fn split_type_and_name(value: &str) -> Result<(String, String)> {
    let value = value.trim();
    if let Some(marker) = value.find("(*") {
        let name_start = marker + 2;
        let name_end = value[name_start..]
            .find(')')
            .map(|offset| name_start + offset)
            .with_context(|| format!("function pointer declaration is missing ): {value}"))?;
        let prefix = value[..marker].trim_end();
        let name = value[name_start..name_end].trim().to_string();
        let suffix = &value[name_end + 1..];
        if prefix.is_empty() || name.is_empty() || suffix.is_empty() {
            bail!("function pointer declaration is missing type or name: {value}");
        }
        return Ok((format!("{prefix} (*){suffix}"), name));
    }

    let trimmed = value.trim_end();
    let name_end = trimmed.len();
    let name_start = trimmed[..name_end]
        .rfind(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
        .map_or(0, |index| index + 1);
    let ty = trimmed[..name_start].trim_end().to_string();
    let name = trimmed[name_start..name_end].to_string();
    if ty.is_empty() || name.is_empty() {
        bail!("declaration is missing type or name: {value}");
    }
    Ok((ty, name))
}

fn find_declaration_end(input: &str, start: usize) -> Result<usize> {
    input[start..]
        .find(';')
        .map(|offset| start + offset + 1)
        .with_context(|| format!("could not find declaration terminator after byte {start}"))
}

fn find_typedef_declaration_end(input: &str, start: usize) -> Result<usize> {
    let closing_brace = input[start..]
        .find("\n}")
        .map(|offset| start + offset + 1)
        .with_context(|| format!("could not find typedef closing brace after byte {start}"))?;
    input[closing_brace..]
        .find(';')
        .map(|offset| closing_brace + offset + 1)
        .with_context(|| format!("could not find typedef terminator after byte {start}"))
}

fn text_between<'a>(input: &'a str, start: &str, end: &str) -> Result<&'a str> {
    let start_index = input
        .find(start)
        .with_context(|| format!("missing start marker: {start}"))?
        + start.len();
    let end_index = input[start_index..]
        .find(end)
        .with_context(|| format!("missing end marker: {end}"))?
        + start_index;
    Ok(&input[start_index..end_index])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn parses_pointer_type_with_name() {
        let parameter = parse_named_type("const char *message").unwrap();
        assert_eq!(parameter.ty, "const char *");
        assert_eq!(parameter.name, "message");
    }

    #[test]
    fn parses_plain_type_with_name() {
        let parameter = parse_named_type("uint64_t handle").unwrap();
        assert_eq!(parameter.ty, "uint64_t");
        assert_eq!(parameter.name, "handle");
    }

    #[test]
    fn emits_multiline_function_prototype() {
        let mut output = String::new();
        push_function_declaration(
            &mut output,
            "bool",
            "vesper_runtime_example",
            &[
                Parameter {
                    ty: "uint64_t".to_string(),
                    name: "handle".to_string(),
                },
                Parameter {
                    ty: "const char *".to_string(),
                    name: "message".to_string(),
                },
            ],
            &FunctionStorage::Public,
        )
        .unwrap();
        assert_eq!(
            output,
            "bool vesper_runtime_example(\n    uint64_t handle,\n    const char *message);\n"
        );
    }

    #[test]
    fn parses_function_pointer_field() {
        let parameter =
            parse_named_type("void (*on_progress)(void *context, float ratio)").unwrap();
        assert_eq!(parameter.ty, "void (*)(void *context, float ratio)");
        assert_eq!(parameter.name, "on_progress");

        let mut output = String::new();
        push_typed_name(&mut output, &parameter.ty, &parameter.name);
        assert_eq!(output, "void (*on_progress)(void *context, float ratio)");
    }

    #[test]
    fn rejects_wrapper_manifest_with_missing_ffi_symbol_call() {
        let temp_dir = std::env::temp_dir().join(format!(
            "vesper-shim-generator-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(temp_dir.join("fragments/wrappers")).unwrap();
        std::fs::write(
            temp_dir.join("fragments/wrappers/vesper_runtime_example.c.inc"),
            "{\n  return true;\n}\n",
        )
        .unwrap();

        let manifest = Manifest {
            header_guard: "VESPER_PLAYER_KIT_BRIDGE_SHIM_H".to_string(),
            header_includes: vec!["<stdbool.h>".to_string()],
            c_includes: vec!["\"include/VesperPlayerKitBridgeShim.h\"".to_string()],
            public_declarations: vec![Declaration::Function {
                return_type: "bool".to_string(),
                name: "vesper_runtime_example".to_string(),
                parameters: Vec::new(),
                storage: FunctionStorage::Public,
            }],
            private_declarations: Vec::new(),
            source_items: vec![SourceItem::Wrapper {
                function: "vesper_runtime_example".to_string(),
                ffi_symbols: vec!["player_ffi_missing".to_string()],
                ownership: Vec::new(),
                body_fragment: "fragments/wrappers/vesper_runtime_example.c.inc".to_string(),
            }],
        };

        let error = generate_source(&manifest, &temp_dir).unwrap_err();
        let _ = std::fs::remove_dir_all(&temp_dir);
        assert!(error.to_string().contains("player_ffi_missing"));
    }
}
