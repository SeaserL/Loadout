// SPDX-License-Identifier: Apache-2.0

//! Model addressing (ADR-010): `loadout/<name>`, `<provider>/<account>/<model>`,
//! `<provider>/<model>`. Model ids may themselves contain `/`.

/// The provider slug reserved for virtual models.
pub const RESERVED_PROVIDER_SLUG: &str = "loadout";

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum NameError {
    #[error("{what} must be 1-32 characters of a-z, 0-9 and '-', starting with a letter or digit")]
    InvalidFormat { what: &'static str },
    #[error("\"{0}\" is reserved")]
    Reserved(String),
    #[error(
        "account alias \"{alias}\" is ambiguous: model \"{model}\" starts with \"{alias}/\"; choose another alias"
    )]
    AmbiguousAlias { alias: String, model: String },
}

fn valid_ident(s: &str) -> bool {
    let bytes = s.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 32
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

pub fn validate_slug(slug: &str) -> Result<(), NameError> {
    if !valid_ident(slug) {
        return Err(NameError::InvalidFormat {
            what: "provider slug",
        });
    }
    if slug == RESERVED_PROVIDER_SLUG {
        return Err(NameError::Reserved(slug.to_owned()));
    }
    Ok(())
}

/// Validates an account alias against the provider's model ids (ADR-010 §3 ambiguity guard).
pub fn validate_alias<'a>(
    alias: &str,
    provider_model_ids: impl IntoIterator<Item = &'a str>,
) -> Result<(), NameError> {
    if !valid_ident(alias) {
        return Err(NameError::InvalidFormat {
            what: "account alias",
        });
    }
    for model in provider_model_ids {
        if model.split('/').next() == Some(alias) && model.contains('/') {
            return Err(NameError::AmbiguousAlias {
                alias: alias.to_owned(),
                model: model.to_owned(),
            });
        }
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParsedName<'a> {
    Virtual(&'a str),
    /// `provider/rest` — `rest` is either `account/model` or just `model`; the table decides
    /// using the provider's known aliases.
    Provider {
        provider: &'a str,
        rest: &'a str,
    },
}

pub fn parse_name(name: &str) -> Option<ParsedName<'_>> {
    let (head, rest) = name.split_once('/')?;
    if head.is_empty() || rest.is_empty() {
        return None;
    }
    if head == RESERVED_PROVIDER_SLUG {
        return Some(ParsedName::Virtual(rest));
    }
    Some(ParsedName::Provider {
        provider: head,
        rest,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_and_aliases() {
        assert!(validate_slug("opencode-go").is_ok());
        assert!(validate_slug("my-vllm2").is_ok());
        for bad in ["", "-x", "Upper", "a_b", "a/b", &"x".repeat(33)] {
            assert!(validate_slug(bad).is_err(), "{bad}");
        }
        assert_eq!(
            validate_slug("loadout"),
            Err(NameError::Reserved("loadout".into()))
        );

        assert!(validate_alias("main", ["gpt-4o", "anthropic/claude"]).is_ok());
        assert!(matches!(
            validate_alias("anthropic", ["gpt-4o", "anthropic/claude"]),
            Err(NameError::AmbiguousAlias { .. })
        ));
        // A model named exactly like the alias (no slash) is not ambiguous.
        assert!(validate_alias("work", ["work"]).is_ok());
    }

    #[test]
    fn parses_names() {
        assert_eq!(
            parse_name("loadout/realtime"),
            Some(ParsedName::Virtual("realtime"))
        );
        assert_eq!(
            parse_name("openrouter/main/anthropic/claude"),
            Some(ParsedName::Provider {
                provider: "openrouter",
                rest: "main/anthropic/claude"
            })
        );
        assert_eq!(parse_name("gpt-4o"), None);
        assert_eq!(parse_name("/x"), None);
        assert_eq!(parse_name("x/"), None);
    }
}
