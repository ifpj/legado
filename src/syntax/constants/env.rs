use super::Constant;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

const SMALL_SCOPE: usize = 32;
const MAX_LAYERS: usize = 8;

/// Immutable inherited bindings plus a small, copy-on-write local overlay.
/// Function arguments must not copy the entire enclosing program's scope.
#[derive(Clone, Debug, Default)]
pub struct Env<'a> {
    local: Arc<HashMap<String, Constant<'a>>>,
    parent: Option<Arc<Env<'a>>>,
    depth: usize,
}

impl<'a> Env<'a> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, name: &str) -> Option<&Constant<'a>> {
        self.local
            .get(name)
            .or_else(|| self.parent.as_ref()?.get(name))
    }

    pub fn contains_key(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    fn writable(&mut self) -> &mut HashMap<String, Constant<'a>> {
        if Arc::strong_count(&self.local) > 1 && self.local.len() > SMALL_SCOPE {
            if self.depth < MAX_LAYERS {
                let inherited = Arc::new(self.clone());
                self.local = Arc::new(HashMap::new());
                self.depth += 1;
                self.parent = Some(inherited);
            } else {
                // Bound lookup and retained snapshot depth even for unusually
                // nested functions or repeated detached mutations.
                let mut merged = HashMap::new();
                let mut layer = Some(&*self);
                while let Some(env) = layer {
                    for (key, value) in env.local.iter() {
                        if !merged.contains_key(key) {
                            merged.insert(key.clone(), value.clone());
                        }
                    }
                    layer = env.parent.as_deref();
                }
                self.local = Arc::new(merged);
                self.parent = None;
                self.depth = 0;
            }
        }
        Arc::make_mut(&mut self.local)
    }

    pub fn insert(&mut self, name: String, value: Constant<'a>) {
        self.writable().insert(name, value);
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut Constant<'a>> {
        // Copy only the inherited value being changed. Array/object contents
        // keep their own COW boundary, preserving all older scope snapshots.
        if !self.local.contains_key(name) {
            let value = self.parent.as_ref()?.get(name)?.clone();
            self.writable().insert(name.to_owned(), value);
        }
        // writable() may move a shared local map into an inherited layer.
        self.writable();
        if !self.local.contains_key(name) {
            let value = self.parent.as_ref()?.get(name)?.clone();
            Arc::make_mut(&mut self.local).insert(name.to_owned(), value);
        }
        Arc::make_mut(&mut self.local).get_mut(name)
    }

    pub fn values(&self) -> impl Iterator<Item = &Constant<'a>> {
        let mut seen = HashSet::new();
        std::iter::successors(Some(self), |env| env.parent.as_deref())
            .flat_map(|env| env.local.iter())
            .filter_map(move |(name, value)| seen.insert(name).then_some(value))
    }
}

impl<'a> std::ops::Index<&str> for Env<'a> {
    type Output = Constant<'a>;
    fn index(&self, name: &str) -> &Self::Output {
        self.get(name).expect("scope binding")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn large() -> Env<'static> {
        let mut env = Env::new();
        for i in 0..2000 {
            env.insert(format!("v{i}"), Constant::Number(i as f64));
        }
        env
    }

    #[test]
    fn local_parameters_share_large_inherited_scope_without_copying_it() {
        let saved = large();
        let mut local = saved.clone();
        local.insert("argument".into(), Constant::Number(7.0));
        local.insert("v5".into(), Constant::Dynamic);
        assert_eq!(local.local.len(), 2);
        assert!(Arc::ptr_eq(
            &saved.local,
            &local.parent.as_ref().unwrap().local
        ));
        assert_eq!(local["v1999"].integer(), Ok(1999));
        assert!(matches!(local["v5"], Constant::Dynamic));
        assert_eq!(saved["v5"].integer(), Ok(5));
        assert!(!saved.contains_key("argument"));
        assert_eq!(local.values().count(), 2001);
    }

    #[test]
    fn deep_scope_overlays_flatten_with_latest_binding_and_saved_views_intact() {
        let mut env = large();
        let mut saved = Vec::new();
        for generation in 0..24 {
            saved.push(env.clone());
            for i in 0..40 {
                env.insert(format!("v{i}"), Constant::Number(generation as f64));
            }
            assert!(env.depth <= MAX_LAYERS);
            assert_eq!(env.values().count(), 2000);
        }
        for (i, snapshot) in saved.iter().enumerate().skip(1) {
            assert_eq!(snapshot["v0"].integer(), Ok(i as i64 - 1));
        }
        assert_eq!(saved[0]["v39"].integer(), Ok(39));
        assert_eq!(env["v0"].integer(), Ok(23));
        assert_eq!(env["v1999"].integer(), Ok(1999));
    }

    #[test]
    fn inherited_array_mutation_detaches_both_binding_and_array() {
        let mut base = large();
        base.insert(
            "table".into(),
            Constant::Array(Arc::new(vec![Constant::Number(3.0)])),
        );
        let saved = base.clone();
        let mut inherited = base.clone();
        inherited.insert("argument".into(), Constant::Null);
        for env in [&mut base, &mut inherited] {
            let Some(Constant::Array(values)) = env.get_mut("table") else {
                panic!("array");
            };
            Arc::make_mut(values)[0] = Constant::Number(9.0);
        }
        let Constant::Array(values) = &saved["table"] else {
            panic!("array");
        };
        assert_eq!(values[0].integer(), Ok(3));
        assert!(inherited.get_mut("absent").is_none());
    }
}
