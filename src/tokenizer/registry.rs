//! Model-name → tokenizer dispatch — lite build (estimation only).

use super::{EstimatingCounter, Tokenizer};

/// Which family of tokenizer was selected for a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// Character-density estimation (chars/token formula).
    Estimation,
}

/// Pick a backend purely from the model name.
pub fn detect_backend(_model: &str) -> Backend {
    Backend::Estimation
}

/// Return a tokenizer for `model`. Uses calibrated estimation per family.
pub fn get_tokenizer(model: &str) -> Box<dyn Tokenizer> {
    Box::new(default_estimator_for(model))
}

fn default_estimator_for(model: &str) -> EstimatingCounter {
    let m = model.to_ascii_lowercase();
    if m.starts_with("claude-") {
        EstimatingCounter::new(3.5)
    } else if m.starts_with("gemini") || m.starts_with("palm") || m.starts_with("command") {
        EstimatingCounter::new(4.0)
    } else {
        EstimatingCounter::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimator_density_per_family() {
        let claude = get_tokenizer("claude-3-opus");
        assert_eq!(claude.count_text(&"a".repeat(35)), 10);

        let gemini = get_tokenizer("gemini-1.5-pro");
        assert_eq!(gemini.count_text(&"a".repeat(40)), 10);
    }
}
