//! Settings for [`Engine`]'s limitations.

use crate::Engine;
use std::num::{NonZeroU64, NonZeroUsize};

/// Default limits, identical in debug and release builds (the hot-reload runner
/// is a debug build; a script must not behave differently there).
pub mod default_limits {
    /// Maximum levels of function calls.
    ///
    /// Debug-build call frames are large; deeper recursion there needs a bigger
    /// evaluation stack first.
    #[cfg(debug_assertions)]
    pub const MAX_CALL_STACK_DEPTH: usize = 8;
    /// Maximum levels of function calls (release builds).
    #[cfg(not(debug_assertions))]
    pub const MAX_CALL_STACK_DEPTH: usize = 64;
    /// Maximum levels of expressions.
    pub const MAX_EXPR_DEPTH: usize = 256;
    /// Maximum levels of expressions in function bodies.
    pub const MAX_FUNCTION_EXPR_DEPTH: usize = 256;
}

/// A type containing all the limits imposed by the [`Engine`].
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct Limits {
    /// Maximum levels of call-stack to prevent infinite recursion.
    ///
    /// Set to zero to effectively disable function calls.
    pub call_stack_depth: usize,
    /// Maximum depth of statements/expressions at global level.
    pub expr_depth: Option<NonZeroUsize>,
    /// Maximum depth of statements/expressions in functions.
    pub function_expr_depth: Option<NonZeroUsize>,
    /// Maximum number of operations allowed to run.
    pub num_operations: Option<NonZeroU64>,
    /// Maximum number of variables allowed at any instant.
    ///
    /// Set to zero to effectively disable creating variables.
    pub num_variables: usize,
    /// Maximum number of scripted functions allowed.
    ///
    /// Set to zero to effectively disable defining any function.
    pub num_functions: usize,
    /// Maximum number of [modules][crate::Module] allowed to load.
    ///
    /// Set to zero to effectively disable loading any [module][crate::Module].
    pub num_modules: usize,
    /// Maximum length of a [string][crate::ImmutableString].
    pub string_len: Option<NonZeroUsize>,
    /// Maximum length of an [array][crate::Array].
    pub array_size: Option<NonZeroUsize>,
    /// Maximum number of properties in an [object map][crate::Map].
    pub map_size: Option<NonZeroUsize>,
}

impl Limits {
    /// Create a new [`Limits`] with default values.
    #[inline]
    pub const fn new() -> Self {
        Self {
            call_stack_depth: default_limits::MAX_CALL_STACK_DEPTH,
            expr_depth: NonZeroUsize::new(default_limits::MAX_EXPR_DEPTH),
            function_expr_depth: NonZeroUsize::new(default_limits::MAX_FUNCTION_EXPR_DEPTH),
            num_operations: None,
            num_variables: usize::MAX,
            num_functions: usize::MAX,
            num_modules: usize::MAX,
            string_len: None,
            array_size: None,
            map_size: None,
        }
    }
}

impl Default for Limits {
    #[inline(always)]
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    /// Is there a data size limit set?
    #[inline(always)]
    pub(crate) const fn has_data_size_limit(&self) -> bool {
        self.limits.string_len.is_some()
            || {
                {
                    self.limits.array_size.is_some()
                }
            }
            || {
                {
                    self.limits.map_size.is_some()
                }
            }
    }
    /// Set the maximum levels of function calls allowed for a script in order to avoid
    /// infinite recursion and stack overflows.
    #[inline(always)]
    pub fn set_max_call_levels(&mut self, levels: usize) -> &mut Self {
        self.limits.call_stack_depth = levels;
        self
    }
    /// The maximum levels of function calls allowed for a script.
    #[inline(always)]
    #[must_use]
    pub const fn max_call_levels(&self) -> usize {
        self.limits.call_stack_depth
    }
    /// Set the maximum number of operations allowed for a script to run to avoid
    /// consuming too much resources (0 for unlimited).
    #[inline(always)]
    pub fn set_max_operations(&mut self, operations: u64) -> &mut Self {
        self.limits.num_operations = NonZeroU64::new(operations);
        self
    }
    /// The maximum number of operations allowed for a script to run (0 for unlimited).
    #[inline]
    #[must_use]
    pub const fn max_operations(&self) -> u64 {
        match self.limits.num_operations {
            Some(n) => n.get(),
            None => 0,
        }
    }
    /// Set the maximum number of variables allowed for a script at any instant.
    #[inline(always)]
    pub fn set_max_variables(&mut self, variables: usize) -> &mut Self {
        self.limits.num_variables = variables;
        self
    }
    /// The maximum number of variables allowed for a script at any instant.
    #[inline(always)]
    #[must_use]
    pub const fn max_variables(&self) -> usize {
        self.limits.num_variables
    }
    /// Set the maximum number of scripted functions allowed for a script at any instant.
    #[inline(always)]
    pub fn set_max_functions(&mut self, functions: usize) -> &mut Self {
        self.limits.num_functions = functions;
        self
    }
    /// The maximum number of scripted functions allowed for a script at any instant.
    #[inline(always)]
    #[must_use]
    pub const fn max_functions(&self) -> usize {
        self.limits.num_functions
    }
    /// Set the maximum number of imported [modules][crate::Module] allowed for a script.
    #[inline(always)]
    pub fn set_max_modules(&mut self, modules: usize) -> &mut Self {
        self.limits.num_modules = modules;
        self
    }
    /// The maximum number of imported [modules][crate::Module] allowed for a script.
    #[inline(always)]
    #[must_use]
    pub const fn max_modules(&self) -> usize {
        self.limits.num_modules
    }
    /// Set the depth limits for expressions (0 for unlimited).
    #[inline(always)]
    pub fn set_max_expr_depths(
        &mut self,
        max_expr_depth: usize,
        max_function_expr_depth: usize,
    ) -> &mut Self {
        self.limits.expr_depth = NonZeroUsize::new(max_expr_depth);
        {
            self.limits.function_expr_depth = NonZeroUsize::new(max_function_expr_depth);
        }
        self
    }
    /// The depth limit for expressions (0 for unlimited).
    #[inline]
    #[must_use]
    pub const fn max_expr_depth(&self) -> usize {
        match self.limits.expr_depth {
            Some(n) => n.get(),
            None => 0,
        }
    }
    /// The depth limit for expressions in functions (0 for unlimited).
    #[inline]
    #[must_use]
    pub const fn max_function_expr_depth(&self) -> usize {
        match self.limits.function_expr_depth {
            Some(n) => n.get(),
            None => 0,
        }
    }
    /// Set the maximum length, in bytes, of [strings][crate::ImmutableString] (0 for unlimited).
    #[inline(always)]
    pub fn set_max_string_size(&mut self, max_len: usize) -> &mut Self {
        self.limits.string_len = NonZeroUsize::new(max_len);
        self
    }
    /// The maximum length, in bytes, of [strings][crate::ImmutableString] (0 for unlimited).
    #[inline]
    #[must_use]
    pub const fn max_string_size(&self) -> usize {
        match self.limits.string_len {
            Some(n) => n.get(),
            None => 0,
        }
    }
    /// Set the maximum length of [arrays][crate::Array] (0 for unlimited).
    #[inline(always)]
    pub fn set_max_array_size(&mut self, max_size: usize) -> &mut Self {
        self.limits.array_size = NonZeroUsize::new(max_size);
        self
    }
    /// The maximum length of [arrays][crate::Array] (0 for unlimited).
    #[inline]
    #[must_use]
    pub const fn max_array_size(&self) -> usize {
        match self.limits.array_size {
            Some(n) => n.get(),
            None => 0,
        }
    }
    /// Set the maximum size of [object maps][crate::Map] (0 for unlimited).
    #[inline(always)]
    pub fn set_max_map_size(&mut self, max_size: usize) -> &mut Self {
        self.limits.map_size = NonZeroUsize::new(max_size);
        self
    }
    /// The maximum size of [object maps][crate::Map] (0 for unlimited).
    #[inline]
    #[must_use]
    pub const fn max_map_size(&self) -> usize {
        match self.limits.map_size {
            Some(n) => n.get(),
            None => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Engine;

    #[test]
    fn deeply_nested_builder_chains_compile() {
        // Deep parses need a main-thread-sized stack, not the 2 MiB test-thread default.
        std::thread::Builder::new()
            .stack_size(8 << 20)
            .spawn(deeply_nested_builder_chains)
            .unwrap()
            .join()
            .unwrap();
    }

    fn deeply_nested_builder_chains() {
        let mut view = String::from("Node::new(0)");
        for _ in 0..40 {
            view = format!("Node::new(1).child({view}).pad(1).width(1)");
        }
        let source = format!(
            "struct Node {{ size: i64 }}
             impl Node {{
                 fn new(size: i64) -> Node {{ Node {{ size }} }}
                 fn child(&self, child: Node) -> Node {{ Node {{ size: self.size + child.size }} }}
                 fn pad(&self, by: i64) -> Node {{ Node {{ size: self.size + by }} }}
                 fn width(&self, by: i64) -> Node {{ Node {{ size: self.size + by }} }}
                 fn view() -> Node {{ {view} }}
             }}
             Node::view().size"
        );
        let engine = Engine::new();
        let ast = engine.compile(&source).unwrap();
        assert_eq!(engine.eval_ast::<i64>(&ast).unwrap(), 120);
    }
}
