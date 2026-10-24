use super::boundaries::*;
use super::*;

fn format() -> Format {
    serde_json::from_value(json!({"byte_offset":20,"initial_key":70,"state_step":147,"state_multiplier":32,"state_increment":136,"hash_salt":2160202578u32,"char_xor":227,"state_quadratic":105,"char_mode":"before-subtract"})).unwrap()
}
fn with_function<T>(source: &str, run: impl for<'a> FnOnce(&Tables<'a>, Node<'a>) -> T) -> T {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_javascript::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    assert!(!tree.root_node().has_error());
    let tables = Tables::new(tree.root_node(), source).unwrap();
    run(&tables, tree.root_node().named_child(0).unwrap())
}
#[test]
fn primitive_boundaries_require_equality_and_no_additional_reads_or_dynamic_calls() {
    let source="function literals(raw,ip,key,regs){let tag=((raw[ip++]-20)&255)^key^96;let dest=((raw[ip++]-20)&255)^key^17;if(tag===155){regs[dest]=void 0;}else if(tag===16){regs[dest]=null;}else if(!(tag!==105)){regs[dest]=true;}if(tag!==219){regs[dest]=false;}if(tag===42){regs[dest]=raw[ip++];}if(tag===43){unknown();}if(!!(tag!==200)){unknown();}else{regs[dest]=void 0;}}";
    with_function(source, |t, n| {
        assert_eq!(
            empty_literals_in(t, n, &format()).unwrap(),
            vec![9, 112, 168, 251]
        )
    });
}
#[test]
fn jump_recovers_current_address_order_key_mask_and_sequence_tail() {
    let source="function jump(raw,ip,key,regs){let target;target=(unused=regs[0],(((raw[ip++]-20)&255)^key)<<8|(((raw[ip++]-20)&255)^key)<<16|(((raw[ip++]-20)&255)^key));regs[0]=target;regs[1]=((raw[ip]-20)&255)^key^93;}";
    with_function(source, |t, n| {
        let p = jump_in(t, n, 55, &format()).unwrap();
        assert_eq!(p, json!([55,[0,0,0,93],{"b16":1,"b8":0,"b0":2,"key":3}]));
    });
    for bad in [
        source.replace("<<16", "<<15"),
        source.replace("^key^93", "+key+93"),
    ] {
        with_function(&bad, |t, n| assert!(jump_in(t, n, 55, &format()).is_err()));
    }
}
fn branch_source() -> String {
    "function branch(raw,ip,key,regs,test){let a,b,c,target,next,flag,other,otherKey;a=((raw[ip++]-20)&255)^key^17;b=((raw[ip++]-20)&255)^key^104;c=((raw[ip++]-20)&255)^key^170;target=(((raw[ip++]-20)&255)^key)<<16|(((raw[ip++]-20)&255)^key)<<8|(((raw[ip++]-20)&255)^key);next=((raw[ip++]-20)&255)^key^127;flag=((raw[ip++]-20)&255)^key;test?(regs[0]=target,regs[1]=next):flag?(other=(((raw[ip++]-20)&255)^key)<<16|(((raw[ip++]-20)&255)^key)<<8|(((raw[ip++]-20)&255)^key),otherKey=((raw[ip++]-20)&255)^key^127,regs[0]=other,regs[1]=otherKey):regs[0]=ip;}".into()
}
#[test]
fn conditional_branch_binds_both_targets_to_the_same_state_fields() {
    let source = branch_source();
    with_function(&source, |t, n| {
        let p = branch_in(t, n, 7, &format()).unwrap();
        assert_eq!(p[2]["target"], json!([3, 4, 5]));
        assert_eq!(p[2]["alternateTarget"], json!([8, 9, 10]));
        assert_eq!(p[2]["keyMask"], 127);
    });
    for bad in [
        source.replace("regs[0]=other", "regs[2]=other"),
        source.replace("<<16", "<<15"),
        source.replace(":flag?", ":a?"),
    ] {
        with_function(&bad, |t, n| assert!(branch_in(t, n, 7, &format()).is_err()));
    }
}

#[test]
fn conditional_fallthrough_requires_target_and_key_assignments() {
    let source="function conditional(raw,ip,key,regs,test){let input,target,next;input=((raw[ip++]-20)&255)^key;target=(((raw[ip++]-20)&255)^key)<<16|(((raw[ip++]-20)&255)^key)<<8|(((raw[ip++]-20)&255)^key);next=((raw[ip++]-20)&255)^key^127;test?(regs[0]=target,regs[1]=next):regs[0]=ip;}";
    with_function(source, |t, n| {
        assert_eq!(
            fallthrough_in(t, n, 145, &format()).unwrap()[2]["target"],
            json!([1, 2, 3])
        )
    });
    with_function(&source.replace("regs[1]=next", "regs[1]=input"), |t, n| {
        assert!(fallthrough_in(t, n, 145, &format()).is_err())
    });
}

#[test]
fn swapped_conditional_sides_preserve_target_and_key_ownership() {
    let source = branch_source();
    let inverted_flag = source
        .replace(":flag?(", ":!flag?regs[0]=ip:(")
        .replace("):regs[0]=ip;}", ");}");
    with_function(&inverted_flag, |t, n| {
        assert!(branch_in(t, n, 7, &format()).is_ok())
    });
    let source = source
        .replace("test?(regs[0]=target,regs[1]=next):flag?", "!test?flag?")
        .replace(
            ":regs[0]=ip;}",
            ":regs[0]=ip:(regs[0]=target,regs[1]=next);}",
        );
    with_function(&source, |t, n| {
        assert!(branch_in(t, n, 7, &format()).is_ok())
    });
    let source="function conditional(raw,ip,key,regs,test){let input,target,next;input=((raw[ip++]-20)&255)^key;target=(((raw[ip++]-20)&255)^key)<<16|(((raw[ip++]-20)&255)^key)<<8|(((raw[ip++]-20)&255)^key);next=((raw[ip++]-20)&255)^key^127;!test?regs[0]=ip:(regs[0]=target,regs[1]=next);}";
    with_function(source, |t, n| {
        assert_eq!(
            fallthrough_in(t, n, 5, &format()).unwrap()[2]["keyMask"],
            127
        )
    });
}

#[test]
fn operation_groups_require_equal_width_and_mutually_exclusive_paths() {
    let read = "(((raw[ip++]-20)&255)^key)";
    for width in [2, 3] {
        let group = (0..width)
            .map(|i| format!("let arg{i}={read};"))
            .collect::<String>();
        let good=format!("function operation(raw,ip,key,kind,regs){{if(kind===0){{{group}}}else if(kind===1){{{group}}}else{{{group}}}}}");
        with_function(&good, |t, n| {
            uniform_groups(t, n, &format(), 3, width).unwrap()
        });
        let sequential = format!("function operation(raw,ip,key,regs){{{group}{group}{group}}}");
        with_function(&sequential, |t, n| {
            assert!(uniform_groups(t, n, &format(), 3, width).is_err())
        });
        with_function(
            &good.replacen("let arg0=", &format!("let extra={read};let arg0="), 1),
            |t, n| assert!(uniform_groups(t, n, &format(), 3, width).is_err()),
        );
    }
}

#[test]
fn closure_header_derives_a_bounded_capture_count_from_the_current_reader() {
    let helper="function read(state){return state.h[state.i]^((state.h[state.v][state.h[state.j]++]-20)&255);}";
    with_function(helper, |t, n| byte_helper(t, n, &format()).unwrap());
    let wrapped_helper="function read(state){let ops={xor:function(a,b){return a^b;}};return ops.xor(state.h[state.i],((state.h[state.v][state.h[state.j]++]-20)&255));}";
    with_function(wrapped_helper, |t, n| byte_helper(t, n, &format()).unwrap());
    for bad in [helper.replace("&255", "&254"), helper.replace("++", "--")] {
        with_function(&bad, |t, n| assert!(byte_helper(t, n, &format()).is_err()));
    }
    let source=format!("function closure(state,raw,ip,key){{let dest,target,next,flags,count,captures;dest=read(state)^111;target=(((raw[ip++]-20)&255)^key)<<16|(((raw[ip++]-20)&255)^key)<<8|(((raw[ip++]-20)&255)^key);next=read(state)^127;flags=read(state)^119;count=read(state)^80;captures=[];for(let i=0;i<count;i++){{captures[i]=read(state)^state.l;}}}}{helper}");
    with_function(&source, |t, n| {
        assert_eq!(
            closure_in(t, n, 45, &format()).unwrap(),
            json!([45,vec![0;7],{"count":6,"countMask":80,"maxCount":64}])
        )
    });
    let in_increment = source.replace(
        "i++){captures[i]=read(state)^state.l;}",
        "captures[i]=read(state)^state.l,i++);",
    );
    with_function(&in_increment, |t, n| {
        assert!(closure_in(t, n, 45, &format()).is_ok())
    });
    let wrapped = source
        .replace(helper, wrapped_helper)
        .replace("next=read(state)^127", "next=forward(read,state)^127")
        + "function forward(f,s){return f(s);}";
    with_function(&wrapped, |t, n| {
        assert!(closure_in(t, n, 45, &format()).is_ok())
    });
    for bad in [
        source.replace("i<count", "i<=count"),
        source.replace("i++", "i--"),
        source.replace("i=0", "i=1"),
        source.replace("next=read(state)", "next=read(other)"),
    ] {
        with_function(&bad, |t, n| {
            assert!(closure_in(t, n, 45, &format()).is_err())
        });
    }
}

#[test]
fn property_pool_boundary_requires_the_pool_branch_and_two_suffix_bytes() {
    let source=format!("function property(raw,ip){{let tag,pool,a,b;tag=raw[ip++];if(tag!==127){{let data=[{}];}}else{{pool=raw[ip++];}}a=raw[ip++];b=raw[ip++];}}",vec!["raw[ip++]";7].join(","));
    with_function(&source, |t, n| {
        assert_eq!(
            property_pool_in(t, n, 47, &format()).unwrap(),
            json!([47,[],{"marker":127,"width":5}])
        )
    });
    let flattened=format!("function property(raw,ip){{let order='body|a|b'.split('|'),step=0,tag,pool,a,b;for(;;){{switch(order[step++]){{case 'a':a=raw[ip++];continue;case 'body':tag=raw[ip++];if(tag!==127){{let data=[{}];}}else{{let inner='load|done'.split('|'),j=0;for(;;){{switch(inner[j++]){{case 'done':pool=1;continue;case 'load':pool=raw[ip++];continue;}}break;}}}}continue;case 'b':b=raw[ip++];continue;}}break;}}}}",vec!["raw[ip++]";7].join(","));
    with_function(&flattened, |t, n| {
        assert!(property_pool_in(t, n, 47, &format()).is_ok())
    });
    with_function(&flattened.replace("body|a|b", "a|body|b"), |t, n| {
        assert!(property_pool_in(t, n, 47, &format()).is_err())
    });
    for bad in [
        source.replace("!==127", "!==126"),
        source.replace("b=raw[ip++];", ""),
        source.replace("pool=raw[ip++]", "pool=raw[ip++]+raw[ip++]"),
    ] {
        with_function(&bad, |t, n| {
            assert!(property_pool_in(t, n, 47, &format()).is_err())
        });
    }
}
