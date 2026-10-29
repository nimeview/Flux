# Fail fixtures

Each file here should fail compilation or analysis with a diagnostic.

## Regression roots vs stubs

The `regression_all_fail_programs_rejected` test walks every `.fl` file but only treats **roots** as entries:

| File | Role |
|------|------|
| `modules/unknown_export.fl` | Root — imports `math_stub` and uses missing export |
| `modules/math_stub.fl` | Stub — valid module (`// test: stub`) |
| `modules/cyclic_import.fl` | Root — cyclic import entry |
| `modules/cyclic_a.fl`, `cyclic_b.fl` | Stubs — cycle shards (`// test: stub`) |
| Everything else | Root — compiled directly |

Mark new dependency-only modules with `// test: stub` so they are not expected to fail on their own.

## Expected errors

Many files include `// expect: fail Exxx — …` describing the intended diagnostic. Targeted tests in `regression.rs` assert specific error kinds for a subset.
