pub mod check;
pub mod env;
pub mod linearity;
pub mod ty;

use crate::ast::*;
use crate::diagnostics::Diagnostic;

use env::TypeEnv;

/// Type-check an AST.
///
/// Checks:
/// - Type definition validity
/// - Expression type inference within rules
/// - Port protocol: put/take on correct port kinds
/// - Cell linearity: take() must be followed by exactly one put() on every path
/// - peek() is exempt from linearity obligations
pub fn check(source: &SourceFile) -> (TypeEnv, Vec<Diagnostic>) {
    let mut diagnostics = vec![];
    let mut env = TypeEnv::new();

    // First pass: collect type definitions
    env.collect_type_defs(source, &mut diagnostics);

    // Second pass: check each process
    for item in &source.items {
        match &item.node {
            Item::Process(process) => {
                check_process(process, &env, &mut diagnostics);
            }
            Item::Pipe(pipe) => {
                check_pipe(pipe, &env, &mut diagnostics);
            }
            Item::TypeDef(_) => {} // Already processed
        }
    }

    (env, diagnostics)
}

fn check_process(process: &Process, env: &TypeEnv, diagnostics: &mut Vec<Diagnostic>) {
    // Build environment with port bindings
    let mut rule_env = env.clone();

    let mut state_ports = vec![];

    for port in &process.ports {
        let port_ty = env.resolve_type_expr(&port.ty.node, diagnostics);
        rule_env.define(port.name.node.clone(), port_ty);
        if port.kind == PortKind::State {
            state_ports.push(port.name.node.clone());
        }
    }

    // Check each rule
    for rule in &process.rules {
        let mut rule_env = rule_env.clone();
        rule_env.push_scope();

        // Type-check statements
        check::check_stmts(&rule.body, &mut rule_env, diagnostics);

        rule_env.pop_scope();

        // Linearity check for state ports
        linearity::check_rule_linearity(rule, &state_ports, diagnostics);
    }
}

fn check_pipe(pipe: &Pipe, env: &TypeEnv, diagnostics: &mut Vec<Diagnostic>) {
    // Check queue declarations have valid types
    for decl in &pipe.queue_decls {
        env.resolve_type_expr(&decl.ty.node, diagnostics);
    }

    // Check async queue declarations have valid types
    for decl in &pipe.async_queue_decls {
        env.resolve_type_expr(&decl.ty.node, diagnostics);
    }

    // Check instances reference valid names
    for instance in &pipe.instances {
        // TODO: look up process definition and check port bindings
        let _ = &instance.process_name;
    }

    let _ = pipe;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desugar;
    use crate::parse;

    fn typecheck(src: &str) -> Vec<Diagnostic> {
        let (cst, errors) = parse::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let mut desugar_diags = vec![];
        let ast = desugar::desugar(cst.unwrap(), &mut desugar_diags);
        assert!(
            desugar_diags.is_empty(),
            "desugar errors: {desugar_diags:?}"
        );
        let (_env, diags) = check(&ast);
        diags
    }

    #[test]
    fn empty_source_typechecks() {
        let source = SourceFile { items: vec![] };
        let (_env, errors) = check(&source);
        assert!(errors.is_empty());
    }

    #[test]
    fn type_def_ok() {
        let diags = typecheck("type Word = Bits 32");
        assert!(diags.is_empty(), "errors: {diags:?}");
    }

    #[test]
    fn simple_process_ok() {
        let src = r#"
type Word = Bits 32
process Counter {
    state: count : Cell(Bits 32, init = 0)
    rule tick {
        let c = count.take()
        count.put(c + 1)
    }
}
"#;
        let diags = typecheck(src);
        assert!(diags.is_empty(), "errors: {diags:?}");
    }

    #[test]
    fn linearity_error_missing_put() {
        let src = r#"
process Foo {
    state: x : Cell(Bits 32, init = 0)
    rule go {
        let v = x.take()
    }
}
"#;
        let diags = typecheck(src);
        assert!(
            diags.iter().any(|d| d.message.contains("not put back")),
            "expected linearity error, got: {diags:?}"
        );
    }

    #[test]
    fn peek_no_linearity_error() {
        let src = r#"
process Foo {
    peeks: x : Cell(Bits 32)
    rule go {
        let v = x.peek()
    }
}
"#;
        let diags = typecheck(src);
        assert!(diags.is_empty(), "unexpected errors: {diags:?}");
    }

    #[test]
    fn undefined_variable_error() {
        let src = r#"
process Foo {
    state: x : Cell(Bits 32, init = 0)
    rule go {
        let v = x.take()
        x.put(y)
    }
}
"#;
        let diags = typecheck(src);
        assert!(
            diags.iter().any(|d| d.message.contains("undefined")),
            "expected undefined variable error, got: {diags:?}"
        );
    }
}
