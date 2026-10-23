//! Unit tests for the `eval` meta library.
//!
//! These test the [`EvalModule`] / [`EvalFn`] plumbing in isolation — module
//! surface, argument extraction, the tagged `__ast__` vs raw-serialized Ast
//! acceptance paths, error paths, and delegate forwarding — using a mock
//! [`RuntimeFactory`] / [`RuntimeHandle`] so the leaf crate need not pull in
//! the full kit.

use std::cell::RefCell;
use std::rc::Rc;

use handl_language::LangError;
use handl_language::{Ast, AstNode, Language, ast_value, language_value};
use handl_platform::{EvalContext, EvalError, Function, Module, ModuleLookup};
use handl_runtime::{RuntimeFactory, RuntimeHandle};
use serde_json::{Value, json};

use crate::EvalModule;

// ---------------------------------------------------------------------------
// Mock RuntimeFactory / RuntimeHandle
// ---------------------------------------------------------------------------

/// A canned Ast used both as the input to `eval.eval` and (mirrored) as the
/// value the mock handle returns, so tests can verify the result is forwarded
/// verbatim.
fn sample_ast() -> Ast {
    let mut ast = Ast::new();
    ast.push(AstNode::new("value").with_payload(json!(42)));
    ast
}

/// A raw JSON-serialized [`Ast`] value (the non-tagged form accepted by
/// `eval.eval`).
fn raw_ast_arg() -> Value {
    serde_json::to_value(sample_ast()).expect("Ast serializes")
}

/// A tagged `__ast__` value (the form bound to an unevaluated expression).
fn tagged_ast_arg() -> Value {
    ast_value(&sample_ast())
}

struct MockHandle {
    eval_err: Option<LangError>,
    last_ast: Rc<RefCell<Option<Ast>>>,
    canned_result: Value,
    check_result: Option<handl_runtime::TypecheckResult>,
}

impl RuntimeHandle for MockHandle {
    fn lex(&self, _src: &handl_language::Source) -> Result<handl_language::Tokens, LangError> {
        Err(LangError::NotImplemented)
    }

    fn parse(&self, _toks: &handl_language::Tokens) -> Result<Ast, LangError> {
        Err(LangError::NotImplemented)
    }

    fn eval(&self, ast: &Ast) -> Result<Value, LangError> {
        *self.last_ast.borrow_mut() = Some(ast.clone());
        match &self.eval_err {
            Some(e) => Err(e.clone()),
            None => Ok(self.canned_result.clone()),
        }
    }

    fn typecheck(&self, _ast: &Ast) -> Result<handl_runtime::TypecheckResult, LangError> {
        self.check_result
            .clone()
            .ok_or(LangError::NotImplemented)
    }

    fn compile(
        &self,
        _ast: &Ast,
        _target: &str,
    ) -> Result<handl_runtime::CompiledHandle, LangError> {
        Err(LangError::NotImplemented)
    }

    fn run_compiled(&self, _handle: &handl_runtime::CompiledHandle) -> Result<Value, LangError> {
        Err(LangError::NotImplemented)
    }
}

struct MockFactory {
    build_err: Option<LangError>,
    eval_err: Option<LangError>,
    canned_result: Value,
    check_result: Option<handl_runtime::TypecheckResult>,
    last_ast: Rc<RefCell<Option<Ast>>>,
}

impl MockFactory {
    fn new() -> Self {
        Self {
            build_err: None,
            eval_err: None,
            canned_result: json!(42),
            check_result: None,
            last_ast: Rc::new(RefCell::new(None)),
        }
    }

    fn with_check_result(mut self, r: handl_runtime::TypecheckResult) -> Self {
        self.check_result = Some(r);
        self
    }

    fn with_build_err(mut self, e: LangError) -> Self {
        self.build_err = Some(e);
        self
    }

    fn with_eval_err(mut self, e: LangError) -> Self {
        self.eval_err = Some(e);
        self
    }

    fn with_result(mut self, v: Value) -> Self {
        self.canned_result = v;
        self
    }

    fn last_ast(&self) -> Option<Ast> {
        self.last_ast.borrow().clone()
    }
}

impl RuntimeFactory for MockFactory {
    fn build(
        &self,
        _lang: Language,
        _delegate: Option<Rc<dyn ModuleLookup>>,
    ) -> Result<Box<dyn RuntimeHandle>, LangError> {
        if let Some(e) = &self.build_err {
            return Err(e.clone());
        }
        Ok(Box::new(MockHandle {
            eval_err: self.eval_err.clone(),
            last_ast: Rc::clone(&self.last_ast),
            canned_result: self.canned_result.clone(),
            check_result: self.check_result.clone(),
        }))
    }
}

/// A mock delegate [`ModuleLookup`] that resolves a single known name, used to
/// verify the `EvalModule` forwards unknown names to its delegate.
struct DelegateLookup {
    name: &'static str,
}

impl ModuleLookup for DelegateLookup {
    fn lookup(&self, qualified_name: &str) -> Option<&dyn Function> {
        if qualified_name == self.name {
            Some(&DelegateFn)
        } else {
            None
        }
    }
}

struct DelegateFn;

impl Function for DelegateFn {
    fn call(&self, _args: &[Value], _ctx: &mut EvalContext) -> Result<Value, EvalError> {
        Ok(json!("delegate-called"))
    }
}

// ---------------------------------------------------------------------------
// Module surface
// ---------------------------------------------------------------------------

fn make_module(factory: Rc<MockFactory>) -> EvalModule {
    EvalModule::new(factory, None)
}

fn make_module_with_delegate(
    factory: Rc<MockFactory>,
    delegate: Rc<dyn ModuleLookup>,
) -> EvalModule {
    EvalModule::new(factory, Some(delegate))
}

#[test]
fn module_name_is_eval() {
    let module = make_module(Rc::new(MockFactory::new()));
    assert_eq!(module.name(), "eval");
}

#[test]
fn module_function_lookup() {
    let module = make_module(Rc::new(MockFactory::new()));
    assert!(module.function("eval").is_some());
    assert!(module.function("nope").is_none());
}

fn ok_check() -> handl_runtime::TypecheckResult {
    handl_runtime::TypecheckResult {
        typ: json!({"__sym__": {"handle": 1, "name": "Int", "parent": null, "type": null}}),
        diagnostics: Vec::new(),
        node_types: Default::default(),
        signature: Value::Null,
    }
}

#[test]
fn checked_lookup_resolves_flat_and_qualified_name() {
    let module = make_module(Rc::new(MockFactory::new()));
    assert!(ModuleLookup::lookup(&module, "checked").is_some());
    assert!(ModuleLookup::lookup(&module, "eval.checked").is_some());
}

#[test]
fn checked_on_ast_returns_value_and_type() {
    let factory = Rc::new(MockFactory::new().with_check_result(ok_check()));
    let module = make_module(factory);
    let checked = module.function("checked").expect("checked fn");
    let mut ctx = EvalContext::new();
    let out = checked
        .call(&[language_value(&Language::new()), tagged_ast_arg()], &mut ctx)
        .expect("checked on ast");
    assert_eq!(out["value"], json!(42));
    assert_eq!(out["type"]["__sym__"]["name"], "Int");
}

#[test]
fn checked_on_typecheck_error_never_evals() {
    let bad = handl_runtime::TypecheckResult {
        typ: Value::Null,
        diagnostics: vec![handl_runtime::TypecheckDiagnostic {
            severity: "error".into(),
            message: "unbound var".into(),
            span: None,
        }],
        node_types: Default::default(),
        signature: Value::Null,
    };
    let factory = Rc::new(MockFactory::new().with_check_result(bad));
    let module = make_module(factory);
    let checked = module.function("checked").expect("checked fn");
    let mut ctx = EvalContext::new();
    let err = checked
        .call(&[language_value(&Language::new()), tagged_ast_arg()], &mut ctx)
        .unwrap_err();
    assert!(err.to_string().contains("typecheck errors"));
}

#[test]
fn checked_without_typecheck_engine_errors_honestly() {
    // The mock's `typecheck` reports NotImplemented — the checked path
    // propagates it rather than falling back to raw eval.
    let module = make_module(Rc::new(MockFactory::new()));
    let checked = module.function("checked").expect("checked fn");
    let mut ctx = EvalContext::new();
    let err = checked
        .call(&[language_value(&Language::new()), tagged_ast_arg()], &mut ctx)
        .unwrap_err();
    assert!(err.to_string().contains("eval.checked"));
}

#[test]
fn module_functions_lists_eval() {
    let module = make_module(Rc::new(MockFactory::new()));
    let fns = module.functions();
    assert_eq!(fns.len(), 2);
    assert_eq!(fns[0].0, "eval");
    assert_eq!(fns[1].0, "checked");
}

// ---------------------------------------------------------------------------
// ModuleLookup
// ---------------------------------------------------------------------------

#[test]
fn lookup_resolves_flat_and_qualified_name() {
    let module = make_module(Rc::new(MockFactory::new()));
    assert!(module.lookup("eval").is_some());
    assert!(module.lookup("eval.eval").is_some());
    assert!(module.lookup("other").is_none());
}

#[test]
fn lookup_forwards_unknown_names_to_delegate() {
    let delegate = Rc::new(DelegateLookup { name: "user.fn" });
    let module = make_module_with_delegate(Rc::new(MockFactory::new()), delegate);
    assert!(module.lookup("user.fn").is_some());
    assert!(module.lookup("eval").is_some());
    assert!(module.lookup("eval.eval").is_some());
}

#[test]
fn lookup_returns_none_without_delegate_for_unknown_name() {
    let module = make_module(Rc::new(MockFactory::new()));
    assert!(module.lookup("user.fn").is_none());
}

// ---------------------------------------------------------------------------
// EvalFn::call — happy path (raw + tagged Ast forms)
// ---------------------------------------------------------------------------

fn call(module: &EvalModule, args: &[Value]) -> Result<Value, EvalError> {
    let eval_fn = module.function("eval").expect("eval fn present");
    let mut ctx = EvalContext::new();
    eval_fn.call(args, &mut ctx)
}

fn lang_arg() -> Value {
    language_value(&Language::new())
}

#[test]
fn call_with_raw_ast_returns_handle_result() {
    let factory = Rc::new(MockFactory::new().with_result(json!(7)));
    let module = make_module(Rc::clone(&factory));
    let result = call(&module, &[lang_arg(), raw_ast_arg()]).expect("eval must succeed");
    assert_eq!(result, json!(7));
    // The deserialized Ast was forwarded to the handle.
    let forwarded = factory.last_ast().expect("ast was forwarded");
    assert_eq!(forwarded.len(), 1);
    assert_eq!(forwarded.nodes()[0].kind, "value");
}

#[test]
fn call_with_tagged_ast_returns_handle_result() {
    let factory = Rc::new(MockFactory::new().with_result(json!(9)));
    let module = make_module(Rc::clone(&factory));
    let result = call(&module, &[lang_arg(), tagged_ast_arg()]).expect("eval must succeed");
    assert_eq!(result, json!(9));
    // The tagged Ast was extracted via `as_ast` and forwarded.
    let forwarded = factory.last_ast().expect("ast was forwarded");
    assert_eq!(forwarded.len(), 1);
    assert_eq!(forwarded.nodes()[0].kind, "value");
}

// ---------------------------------------------------------------------------
// EvalFn::call — error paths
// ---------------------------------------------------------------------------

#[test]
fn call_with_no_args_errors_about_language() {
    let module = make_module(Rc::new(MockFactory::new()));
    let err = call(&module, &[]).unwrap_err();
    assert!(err.to_string().contains("Language value"), "got: {}", err);
}

#[test]
fn call_with_non_language_first_arg_errors() {
    let module = make_module(Rc::new(MockFactory::new()));
    let err = call(&module, &[json!("not-a-language"), raw_ast_arg()]).unwrap_err();
    assert!(err.to_string().contains("Language value"), "got: {}", err);
}

#[test]
fn call_with_missing_ast_arg_errors() {
    let module = make_module(Rc::new(MockFactory::new()));
    let err = call(&module, &[lang_arg()]).unwrap_err();
    assert!(
        err.to_string().contains("expected (lang, ast)"),
        "got: {}",
        err
    );
}

#[test]
fn call_with_non_deserializable_ast_errors() {
    let module = make_module(Rc::new(MockFactory::new()));
    // A plain string is neither a tagged `__ast__` value nor a valid
    // serialized `Ast` (which is an object with `nodes`).
    let err = call(&module, &[lang_arg(), json!("not-an-ast")]).unwrap_err();
    assert!(err.to_string().contains("eval:"), "got: {}", err);
}

#[test]
fn call_propagates_factory_build_error() {
    let factory = Rc::new(MockFactory::new().with_build_err(LangError::engine("build-boom")));
    let module = make_module(factory);
    let err = call(&module, &[lang_arg(), raw_ast_arg()]).unwrap_err();
    assert!(err.to_string().contains("build-boom"), "got: {}", err);
}

#[test]
fn call_propagates_handle_eval_error() {
    let factory = Rc::new(MockFactory::new().with_eval_err(LangError::engine("eval-boom")));
    let module = make_module(factory);
    let err = call(&module, &[lang_arg(), raw_ast_arg()]).unwrap_err();
    assert!(err.to_string().contains("eval-boom"), "got: {}", err);
}
