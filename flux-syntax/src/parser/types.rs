//! Parsing of type annotations and type names.

use flux_diagnostics::SourceId;

use crate::ast::*;

use super::{parse_ident, span_of, Rule};

pub(super) fn parse_type_ann(
    source_id: SourceId,
    pair: pest::iterators::Pair<'_, Rule>,
) -> TypeName {
    parse_type_name(source_id, pair.into_inner().next().expect("type ann"))
}

pub(super) fn parse_type_name(
    source_id: SourceId,
    pair: pest::iterators::Pair<'_, Rule>,
) -> TypeName {
    let span = span_of(source_id, &pair);
    match pair.as_rule() {
        Rule::simple_type => TypeName::Simple {
            name: pair.as_str().to_string(),
            span,
        },
        Rule::array_type => {
            let inner = parse_type_name(source_id, pair.into_inner().next().expect("array inner"));
            TypeName::Array {
                inner: Box::new(inner),
                span,
            }
        }
        Rule::option_type => {
            let inner = parse_type_name(source_id, pair.into_inner().next().expect("option inner"));
            TypeName::Option {
                inner: Box::new(inner),
                span,
            }
        }
        Rule::result_type => {
            let mut inner = pair.into_inner();
            let ok = parse_type_name(source_id, inner.next().expect("result ok"));
            let err = parse_type_name(source_id, inner.next().expect("result err"));
            TypeName::Result {
                ok: Box::new(ok),
                err: Box::new(err),
                span,
            }
        }
        Rule::generic_type => {
            let mut inner = pair.into_inner();
            let name = parse_ident(source_id, inner.next().expect("generic name")).name;
            let args = inner.map(|p| parse_type_name(source_id, p)).collect();
            TypeName::Generic { name, args, span }
        }
        Rule::type_name => {
            parse_type_name(source_id, pair.into_inner().next().expect("type inner"))
        }
        _ => TypeName::Simple {
            name: pair.as_str().to_string(),
            span,
        },
    }
}
