use serde::Deserialize;

pub const SHADERS: [&str; 11] = [
    include_str!("../shaders/anime4k/00.wgsl"),
    include_str!("../shaders/anime4k/01.wgsl"),
    include_str!("../shaders/anime4k/02.wgsl"),
    include_str!("../shaders/anime4k/03.wgsl"),
    include_str!("../shaders/anime4k/04.wgsl"),
    include_str!("../shaders/anime4k/05.wgsl"),
    include_str!("../shaders/anime4k/06.wgsl"),
    include_str!("../shaders/anime4k/07.wgsl"),
    include_str!("../shaders/anime4k/08.wgsl"),
    include_str!("../shaders/anime4k/09.wgsl"),
    include_str!("../shaders/anime4k/10.wgsl"),
];

#[derive(Debug, Deserialize)]
pub struct Graph {
    pub textures: Vec<Texture>,
    pub passes: Vec<Pass>,
    pub output: usize,
}

#[derive(Debug, Deserialize)]
pub struct Texture {
    pub scale: [u32; 2],
}

#[derive(Debug, Deserialize)]
pub struct Binding {
    pub binding: u32,
    pub texture: Option<usize>,
    pub sampler: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Pass {
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

impl Graph {
    pub fn load() -> Result<Self, serde_json::Error> {
        serde_json::from_str(include_str!("../shaders/anime4k/graph.json"))
    }
}

impl Pass {
    pub fn bindings(&self) -> &[Binding] {
        match self {
            Self::Compute { bindings, .. } | Self::Render { bindings, .. } => bindings,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_graph_is_the_tauri_cnnx2m_model_with_the_original_residual_overlay() {
        let graph = Graph::load().unwrap();
        let manifest: serde_json::Value =
            serde_json::from_str(include_str!("../shaders/anime4k/graph.json")).unwrap();
        assert_eq!(manifest["package"], "anime4k-webgpu");
        assert_eq!(manifest["version"], "1.0.0");
        assert_eq!(manifest["sourceSha256"].as_str().unwrap().len(), 64);
        assert_eq!(manifest["shaders"].as_array().unwrap().len(), SHADERS.len());
        assert_eq!(graph.passes.len(), 10);
        assert_eq!(graph.textures[graph.output].scale, [2, 2]);
        for shader in SHADERS {
            assert!(!shader.is_empty());
        }
        let Pass::Render { bindings, .. } = graph.passes.last().unwrap() else {
            panic!("CNN output must be combined with the original frame");
        };
        assert_eq!(bindings[1].texture, Some(0));
        assert_eq!(bindings[2].texture, Some(9));
    }
}
