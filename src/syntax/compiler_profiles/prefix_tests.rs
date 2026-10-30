use super::*;

const IV: &str =
    "1779033703,3144134277,1013904242,2773480762,1359893119,2600822924,~-528734636,1541459225";

fn helper(name: &str, stride: &str, choice: &str) -> String {
    format!("function {name}(position,word){{let state=[{IV}],offset=position*({stride});return {choice};}}")
}
fn recover(source: &str) -> Result<Vec<u8>> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_javascript::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    assert!(!tree.root_node().has_error());
    let tables = Tables::new(tree.root_node(), source).unwrap();
    let functions: Vec<_> = children(tree.root_node())
        .into_iter()
        .filter(|n| n.kind() == "function_declaration")
        .collect();
    dispatcher_prefix(&tables, &functions)
}

#[test]
fn equivalent_chunk_expressions_recover_the_current_prefix() {
    // ~-1025 is the actual failing shape; the XOR forms occur in the same
    // sampled helper. No decimal "1024" substring is required.
    for stride in [
        "1024",
        "~-1025",
        "22746.58^23770.39",
        "25150^26174",
        "1<<10",
    ] {
        let source = helper("hash", stride, "word===0?16909060:84281096");
        assert_eq!(
            recover(&source).unwrap(),
            [1, 2, 3, 4, 5, 6, 7, 8],
            "{stride}"
        );
        let changed = source
            .replace("16909060", "151653132")
            .replace("84281096", "219025168");
        assert_eq!(recover(&changed).unwrap(), [9, 10, 11, 12, 13, 14, 15, 16]);
    }
    let reversed = helper(
        "hash",
        "~-1025",
        "!!(word!==(29284^29284))?84281096:16909060",
    );
    assert_eq!(recover(&reversed).unwrap(), [1, 2, 3, 4, 5, 6, 7, 8]);
}

#[test]
fn iv_expressions_and_bound_chunk_constants_are_matched_by_value() {
    let source = helper("hash", "stride", "word?84281096:16909060")
        .replace("let state", "const stride=~-1025;let state")
        .replace("1779033703", "0x6a09e667")
        .replace("3144134277", "-1150833019");
    assert_eq!(recover(&source).unwrap(), [1, 2, 3, 4, 5, 6, 7, 8]);
    let scalar =
        "function digest(word){var iv=0x6a09e667,block=~-1025;return word===0?16909060:84281096}";
    assert_eq!(recover(scalar).unwrap(), [1, 2, 3, 4, 5, 6, 7, 8]);
}

#[test]
fn wrong_chunk_size_iv_and_nested_or_text_decoys_are_rejected() {
    let valid = helper("hash", "~-1025", "word===0?16909060:84281096");
    for source in [
        valid.replace("~-1025", "1025/*1024*/"),
        valid.replace("~-1025", "1024.5"),
        valid.replace("~-1025", "missingChunk"),
        valid.replace("3144134277", "3144134278"),
        "function decoy(word){let label='1779033703 1024';return word?16909060:84281096}".into(),
        format!("function outer(){{{valid}}}"),
    ] {
        let error = recover(&source).unwrap_err();
        assert_eq!(error.message, "Dispatcher BLAKE3 prefix");
        assert_eq!(error.details["hashHelpers"], 0);
        assert_eq!(error.details["prefixCandidates"], 0);
    }
}

#[test]
fn prefix_ambiguity_and_non_discriminating_conditions_still_fail() {
    let valid = helper("hash", "~-1025", "word===0?16909060:84281096");
    let other = helper("other", "1024", "word===0?16909061:84281096");
    let error = recover(&(valid.clone() + &other)).unwrap_err();
    assert_eq!(error.details["hashHelpers"], 2);
    assert_eq!(error.details["prefixCandidates"], 2);
    let duplicate = valid.replace("hash", "same");
    assert_eq!(
        recover(&(valid + &duplicate)).unwrap(),
        [1, 2, 3, 4, 5, 6, 7, 8]
    );
    for condition in ["word>=0", "word===2", "unknown+word===0", "true"] {
        let error = recover(&helper(
            "hash",
            "~-1025",
            &format!("{condition}?16909060:84281096"),
        ))
        .unwrap_err();
        assert_eq!(error.details["hashHelpers"], 1);
        assert_eq!(error.details["prefixCandidates"], 0);
    }
}
