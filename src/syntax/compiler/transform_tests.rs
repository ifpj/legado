use super::*;
fn read(source: &str) -> Result<String> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_javascript::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    transform_name(&Tables::new(tree.root_node(), source)?)
}
#[test]
fn wire_key_slice_can_be_initialized_by_a_declaration() {
    let source="function wrap(selector,fn,data){switch(selector){case 7:return fn(data)}}function encode(input,start){var buf=input.slice(start,start+16);buf=wrap(7,WireKey,buf);}";
    assert_eq!(read(source), Ok("WireKey".into()));
    assert_eq!(
        read(&source.replace("var buf=", "var WireKey;var buf=")),
        Ok("WireKey".into())
    );
    for bad in [
        source.replace("start+16", "start+15"),
        source.replace("WireKey,buf", "WireKey,other"),
        source.replace("buf=wrap", "other=wrap"),
        source.replace("start+16", "missing+16"),
    ] {
        assert!(read(&bad).is_err());
    }
}
