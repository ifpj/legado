//! Crate-level regression suites: historical contracts and native checks.
//! Both suites address every stage by its short module name.
#![allow(unused_imports)]
use crate::*;
use crate::{app::*, program::*, protocol::*, sites::*, syntax::*, tasks::*};
mod migration;
mod native;
