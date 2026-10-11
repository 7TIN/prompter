#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Style {
    Casual,
    Brevity,
    Short,
    Direct,
    Informed,
    Normal,
}

impl Style {
    pub fn all() -> &'static [Style] {
        &[
            Style::Casual,
            Style::Brevity,
            Style::Short,
            Style::Direct,
            Style::Informed,
            Style::Normal,
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            Style::Casual => "Casual",
            Style::Brevity => "Brevity",
            Style::Short => "Short",
            Style::Direct => "Direct",
            Style::Informed => "Informed",
            Style::Normal => "Normal",
        }
    }

    pub fn id(&self) -> &'static str {
        match self {
            Style::Casual => "style-casual",
            Style::Brevity => "style-brevity",
            Style::Short => "style-short",
            Style::Direct => "style-direct",
            Style::Informed => "style-informed",
            Style::Normal => "style-normal",
        }
    }

    pub fn instruction(&self) -> &'static str {
        match self {
            Style::Casual => {
                "Rewrite in a friendly, casual tone. Fix grammar. Keep meaning. Output only the rewrite."
            }
            Style::Brevity => {
                "Rewrite with maximum brevity. Fix grammar. Replace long explanations with the precise technical term or concept in one word where possible. Output only the rewrite."
            }
            Style::Short => {
                "Rewrite as short as possible, ideally 1-2 sentences. Fix grammar. Output only the rewrite."
            }
            Style::Direct => {
                "Rewrite in a direct, no-fluff, imperative style. Fix grammar. Output only the rewrite."
            }
            Style::Informed => {
                "Rewrite as a precise, well-informed technical prompt. Fix grammar, expand ambiguous terms to correct technical concepts. Output only the rewrite."
            }
            Style::Normal => {
                "Rewrite with correct grammar, clear and neutral. Keep meaning and detail level. Output only the rewrite."
            }
        }
    }
}

impl Default for Style {
    fn default() -> Self {
        Style::Brevity
    }
}
