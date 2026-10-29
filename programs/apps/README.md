# Flux sample applications

Small, runnable programs that put Flux's reactive core and standard library to
work on realistic (if minimal) problems, rather than just exercising language
features in isolation.

## Expense Tracker (`expenses.fl`)

A personal budget as a reactive spreadsheet: `total`, `remaining`, and
`over_budget` recompute automatically whenever `items` or `budget` change, and
a `watch` block prints an alert the moment spending exceeds the budget.

```bash
flux run programs/apps/expenses.fl
```

```
--- Flux runtime ---
items = [42, 15, 30, 8]
budget = 100
total = 95
remaining = 5
over_budget = false
```

## Task Board (`tasks.fl`)

An event-driven workflow: `AddTask` and `CompleteTask` events drive the
`pending`/`done` counters through `on` handlers, and `total`/`completion_pct`
stay in sync after every event. Since the CLI runner doesn't take interactive
input yet, drive it from a test or the `flux_runtime::Executor::emit` API (see
`flux_cli/tests/apps.rs` for an example); this is a known gap tracked for the
`repl` `:emit` command in a later milestone.

```bash
flux run programs/apps/tasks.fl
```

```
--- Flux runtime ---
pending = 0
done = 0
total = 0
completion_pct = 0
```

## Sales Rollup (`sales_rollup.fl`)

A small data pipeline built on higher-order functions: `map` applies a tax
rate to every sale, `reduce` folds the taxed amounts into a total, and the
average follows — all three update together if `sales` is ever reassigned.

```bash
flux run programs/apps/sales_rollup.fl
```

```
--- Flux runtime ---
sales = [100, 200, 150, 300, 250]
taxed = [108, 216, 162, 324, 270]
total = 1080
avg = 216
```

## Tests

`flux_cli/tests/apps.rs` compiles each program and drives its reactive
behavior (budget overruns, task events, tax rollups) with behavioral
assertions, in addition to a `flux run`/compile smoke check.
