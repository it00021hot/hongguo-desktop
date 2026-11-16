use crate::io::*;
use anyhow::{Context, Result, ensure};
use naga::{
    Module, ShaderStage,
    valid::{Capabilities, ValidationFlags, Validator},
};
use serde::Deserialize;
use std::{
    collections::{BTreeSet, HashSet},
    path::Path,
};

const MODEL_DIRECTORY: &str = "crates/hongguo-video-gpu/shaders/enhancement/anime4k";
const GRAPH_PATH: &str = "crates/hongguo-video-gpu/shaders/enhancement/anime4k/graph.json";
const SHA256_LENGTH: usize = 64;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Graph {
    package: String,
    version: String,
    source_sha256: String,
    textures: Vec<Texture>,
    passes: Vec<Pass>,
    output: usize,
    shaders: Vec<Shader>,
}

#[derive(Deserialize)]
struct Texture {
    scale: [u32; 2],
}

#[derive(Deserialize)]
struct Shader {
    file: String,
    sha256: String,
}

#[derive(Deserialize)]
struct Binding {
    binding: u32,
    texture: Option<usize>,
    sampler: Option<bool>,
}

#[derive(Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum Pass {
    Compute {
        shader: usize,
        entry: String,
        bindings: Vec<Binding>,
        workgroup: [u32; 2],
        output: usize,
    },
    Render {
        vertex: usize,
        vertex_entry: String,
        fragment: usize,
        fragment_entry: String,
        bindings: Vec<Binding>,
        vertices: u32,
        output: usize,
    },
}

struct HashUpdate {
    file: String,
    previous: String,
    actual: String,
}

pub fn check(args: &[String]) -> Result<()> {
    ensure!(
        args.is_empty() || args == ["--patch"],
        "Expected enhancement-assets [--patch]"
    );
    let patch = !args.is_empty();
    let original = text(GRAPH_PATH)?;
    let graph: Graph = serde_json::from_str(&original)?;
    ensure!(
        !graph.package.is_empty() && !graph.version.is_empty() && valid_hash(&graph.source_sha256),
        "Missing model provenance"
    );
    ensure!(
        !text(Path::new(MODEL_DIRECTORY).join("LICENSE.md"))?
            .trim()
            .is_empty(),
        "Model license is empty"
    );
    let mut modules = Vec::new();
    let mut names = HashSet::new();
    let mut updates = Vec::new();
    for shader in &graph.shaders {
        ensure!(
            names.insert(shader.file.clone()),
            "Duplicate model shader: {}",
            shader.file
        );
        let path = checked(Path::new(MODEL_DIRECTORY), &shader.file)?;
        let source = text(&path)?.replace("\r\n", "\n");
        let normalized = source
            .split('\n')
            .map(|line| line.trim_end_matches([' ', '\t']))
            .collect::<Vec<_>>()
            .join("\n");
        let actual = hash(normalized.as_bytes());
        ensure!(
            patch || shader.sha256 == actual,
            "Shader SHA-256 mismatch: {} (actual {actual}); review the source and use enhancement-assets --patch to update the graph",
            shader.file
        );
        if shader.sha256 != actual {
            updates.push(HashUpdate {
                file: shader.file.clone(),
                previous: shader.sha256.clone(),
                actual,
            });
        }
        modules.push(validate_shader(&path, &source)?);
    }
    let files: HashSet<String> = list(MODEL_DIRECTORY)?
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e == "wgsl"))
        .map(|p| {
            p.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    ensure!(
        !names.is_empty() && names == files,
        "Model shader inventory does not match graph.json"
    );
    validate_graph(&graph, &modules)?;
    for path in list(
        Path::new(MODEL_DIRECTORY)
            .parent()
            .context("Enhancement shader directory")?,
    )? {
        if path.extension().is_some_and(|e| e == "wgsl") {
            validate_shader(&path, &text(&path)?)?;
        }
    }
    if patch && !updates.is_empty() {
        emit_patch(&original, &updates)?;
    } else {
        eprintln!(
            "Enhancement assets verified: {} shaders, {} passes",
            graph.shaders.len(),
            graph.passes.len()
        );
    }
    Ok(())
}

fn valid_hash(value: &str) -> bool {
    value.len() == SHA256_LENGTH && value.bytes().all(|b| b.is_ascii_hexdigit())
}

fn validate_shader(path: &Path, source: &str) -> Result<Module> {
    let module = naga::front::wgsl::parse_str(source)
        .map_err(|e| anyhow::anyhow!("{}: {}", path.display(), e.emit_to_string(source)))?;
    Validator::new(ValidationFlags::all(), Capabilities::all())
        .validate(&module)
        .map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
    Ok(module)
}

fn entry<'a>(
    modules: &'a [Module],
    index: usize,
    name: &str,
    stage: ShaderStage,
) -> Result<&'a naga::EntryPoint> {
    modules
        .get(index)
        .context("Model shader index is out of range")?
        .entry_points
        .iter()
        .find(|e| e.name == name && e.stage == stage)
        .with_context(|| format!("Missing {stage:?} shader entry: {index}/{name}"))
}

fn validate_graph(graph: &Graph, modules: &[Module]) -> Result<()> {
    ensure!(
        !graph.textures.is_empty() && graph.textures[0].scale == [1, 1],
        "Model input must retain the original scale"
    );
    ensure!(
        graph
            .textures
            .iter()
            .all(|t| t.scale.into_iter().all(|s| s > 0)),
        "Model scale must be positive and integral"
    );
    ensure!(!graph.passes.is_empty(), "Model graph has no passes");
    let mut initialized = HashSet::from([0]);
    for pass in &graph.passes {
        let (bindings, output, shader_indices) = match pass {
            Pass::Compute {
                shader,
                entry: name,
                bindings,
                workgroup,
                output,
            } => {
                let size = entry(modules, *shader, name, ShaderStage::Compute)?.workgroup_size;
                ensure!(
                    size[2] == 1
                        && workgroup.iter().all(|value| *value > 0)
                        && workgroup[0] <= size[0]
                        && workgroup[1] <= size[1],
                    "Model dispatch leaves output pixels uncovered: {shader}/{name}"
                );
                (bindings, *output, vec![*shader])
            }
            Pass::Render {
                vertex,
                vertex_entry,
                fragment,
                fragment_entry,
                bindings,
                vertices,
                output,
            } => {
                entry(modules, *vertex, vertex_entry, ShaderStage::Vertex)?;
                entry(modules, *fragment, fragment_entry, ShaderStage::Fragment)?;
                ensure!(*vertices > 0, "Render pass has no vertices");
                (bindings, *output, vec![*vertex, *fragment])
            }
        };
        ensure!(
            output > 0 && output < graph.textures.len(),
            "Model pass output is out of range"
        );
        validate_bindings(bindings, &shader_indices, modules, output, &initialized)?;
        initialized.insert(output);
    }
    ensure!(
        graph.output != 0 && initialized.contains(&graph.output),
        "Model final output was not produced"
    );
    Ok(())
}

fn validate_bindings(
    bindings: &[Binding],
    indices: &[usize],
    modules: &[Module],
    output: usize,
    initialized: &HashSet<usize>,
) -> Result<()> {
    let mut actual = BTreeSet::new();
    for index in indices {
        for (_, global) in modules[*index].global_variables.iter() {
            if let Some(binding) = &global.binding {
                ensure!(binding.group == 0, "Model uses an unsupported bind group");
                actual.insert(binding.binding);
            }
        }
    }
    let mut declared = BTreeSet::new();
    for binding in bindings {
        ensure!(declared.insert(binding.binding), "Duplicate model binding");
        match (binding.texture, binding.sampler) {
            (Some(texture), None) => ensure!(
                texture == output || initialized.contains(&texture),
                "Model reads a texture before it is produced"
            ),
            (None, Some(true)) => {}
            _ => anyhow::bail!("Model binding must select either a texture or a sampler"),
        }
    }
    ensure!(
        declared == actual,
        "Model bindings differ from WGSL resources"
    );
    Ok(())
}

fn emit_patch(original: &str, updates: &[HashUpdate]) -> Result<()> {
    let lines: Vec<_> = original.lines().collect();
    let mut hunks = Vec::new();
    for update in updates {
        let filename = serde_json::to_string(&update.file)?;
        let previous = serde_json::to_string(&update.previous)?;
        let pair = lines
            .windows(2)
            .find(|pair| {
                pair[0].trim_start().starts_with("\"file\":")
                    && pair[0].contains(&filename)
                    && pair[1].contains(&previous)
            })
            .with_context(|| {
                format!(
                    "Expected adjacent file and SHA-256 fields for {}",
                    update.file
                )
            })?;
        let replacement = pair[1].replace(&update.previous, &update.actual);
        hunks.push(format!("@@\n {}\n-{}\n+{replacement}", pair[0], pair[1]));
    }
    println!("*** Begin Patch\n*** Update File: {GRAPH_PATH}");
    for hunk in hunks {
        println!("{hunk}");
    }
    println!("*** End Patch");
    Ok(())
}
