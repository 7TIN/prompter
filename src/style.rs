use std::fmt;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Style {
    Casual,
    #[default]
    Brevity,
    Short,
    Direct,
    Informed,
    Normal,
}

impl Style {
    pub const ALL: &'static [Self] = &[
        Self::Casual,
        Self::Brevity,
        Self::Short,
        Self::Direct,
        Self::Informed,
        Self::Normal,
    ];

    pub fn all() -> &'static [Self] {
        Self::ALL
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Casual => "Casual",
            Self::Brevity => "Brevity",
            Self::Short => "Short",
            Self::Direct => "Direct",
            Self::Informed => "Informed",
            Self::Normal => "Normal",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::Casual => "style-casual",
            Self::Brevity => "style-brevity",
            Self::Short => "style-short",
            Self::Direct => "style-direct",
            Self::Informed => "style-informed",
            Self::Normal => "style-normal",
        }
    }

    pub fn instruction(self) -> &'static str {
        match self {
            Self::Casual => {
                "Rewrite in a friendly, casual tone. Fix grammar. Keep meaning. Output only the rewrite."
            }
            Self::Brevity => {
                "Rewrite with maximum brevity. Fix grammar. Replace long explanations with the precise technical term or concept in one word where possible. Output only the rewrite."
            }
            Self::Short => {
                "Rewrite as short as possible, ideally 1-2 sentences. Fix grammar. Output only the rewrite."
            }
            Self::Direct => {
                "Rewrite in a direct, no-fluff, imperative style. Fix grammar. Output only the rewrite."
            }
            Self::Informed => {
                "Rewrite as a precise, well-informed technical prompt. Fix grammar, expand ambiguous terms to correct technical concepts. Output only the rewrite."
            }
            Self::Normal => {
                "Rewrite with correct grammar, clear and neutral. Keep meaning and detail level. Output only the rewrite."
            }
        }
    }
}

impl fmt::Display for Style {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}
