#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Backend {
    Platform,
}

impl Backend {
    pub fn decoder_label(self) -> &'static str {
        match self {
            Self::Platform => "platform",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_as_platform() {
        assert_eq!(
            serde_json::to_string(&Backend::Platform).unwrap(),
            "\"platform\""
        );
        assert_eq!(
            serde_json::from_str::<Backend>("\"platform\"").unwrap(),
            Backend::Platform
        );
    }
}
