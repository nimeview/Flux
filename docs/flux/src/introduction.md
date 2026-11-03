# Introduction

Flux is a small, statically typed language with reactive values built in. You write ordinary functions, structs, and enums. When some values depend on others, you can declare that once, and Flux keeps them up to date, the way cells in a spreadsheet do.

Here is both at once:

```flux
import io { println }

event Deposit(amount: Int)

let mut balance = 0

let doubled = balance * 2

on Deposit { amount ->
    balance = balance + amount
}

watch balance {
    println("balance={balance} doubled={doubled}")
}

fn main() {
    emit Deposit(50)
    emit Deposit(25)
}
```

```output
balance=0 doubled=0
balance=50 doubled=100
balance=75 doubled=150
```

`balance` is a source you can change. `doubled` reads it, so it is recomputed whenever `balance` changes. The `watch` runs once at the start and again after each change.

You do not have to use any of this. A Flux program can be plain functions and data, as most of this book's early chapters are.

## What to expect

Flux takes familiar ideas from Rust and similar languages: `struct`, `enum`, `match`, `Result`, and `?`, with types checked before anything runs. It has no ownership or borrowing to learn: values behave as independent copies, and updating an array you alone hold happens in place.

It is young. The version is 0.1, and [Status and limits](./status.md) lists what is stable, what is experimental, and what does not exist yet.

## How this book relates to the specification

This book teaches the language. [`SPEC.md`](https://github.com/nimeview/Flux/blob/main/SPEC.md) in the repository is the exact reference, and where the two seem to differ, the specification is right. Every example in this book is run by the test suite on both of Flux's engines, so the code and output you see here are real.
