fn evaluate_program(program: &mut Program) -> std::io::Result<()> {
    // Retire ordinary top-level support after evaluation. Escaped callbacks
    // retain the same receipt with their captured support for their own lifetime.
    let ast = program
        .ast
        .take()
        .ok_or_else(|| std::io::Error::other("top-level program already evaluated"))?;
    program
        .engine
        .eval_ast::<Dynamic>(&ast)
        .map(|_| ())
        .map_err(|error| std::io::Error::other(error.to_string()))
}
