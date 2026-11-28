"""Small, sequential release-binary probes; never builds or benchmarks."""
from pathlib import Path
import subprocess, json, re

ROOT = Path(__file__).resolve().parents[3]
OUT = Path(__file__).resolve().parent
cases = {
"e002": "let limit = 10\nfn main() -> Int { limit }\n",
"e003": "let mut x = 1\nderived y = x * 2\nact fn main() { y = 4 }\n",
"e008_loop": "fn count() -> Int { let mut n = 0; while n < 3 { n += 1 }; n }\nderived y = count()\n",
"e008_return": "fn count() -> Int { return 3 }\nderived y = count()\n",
"e008_snap": "let mut x = 1\nfn read() -> Int { snap(x) }\nderived y = read()\n",
"e008_action": "let mut x = 1\nact fn read() -> Int { x }\nfn call() -> Int { read() }\nderived y = call()\n",
"e008_const": "act fn read() -> Int { 1 }\nconst Y = read()\n",
"e008_track": "act fn read() -> Int { 1 }\ntrack fn read2() -> Int { read() }\n",
"e008_extern": "extern fn read() -> Int\nderived y = read()\n",
"e008_track_extern": "extern fn read() -> Int\ntrack fn read2() -> Int { read() }\n",
"e010": "derived a: Int = b + 1\nderived b: Int = a + 1\n",
"e010_branch": "derived a: Int = if false { b } else { 1 }\nderived b: Int = a + 1\n",
"e011_watch": "fn main() { watch { } }\n",
"e011_on": "event Tick\nfn main() { on Tick { } }\n",
"e016": "let x = 1\nevent Tick\non Tick { x = 2 }\n",
"e009_local": "fn main() { let x = 1; x = 2 }\n",
"e009_const": "const X = 1\nact fn main() { X = 2 }\n",
"e017_derived": "import io { println }\nfn noisy() -> Int { println(\"again\"); 1 }\nderived x = noisy()\n",
"e017_const": "import io { println }\nconst X = println(\"again\")\n",
"e017_emit": "event Tick\nfn noisy() -> Int { emit Tick(); 1 }\nderived x = noisy()\n",
"e027": "let mut x = 0\nfn main() { x = 1 }\n",
"e027_track": "let mut x = 0\ntrack fn f() { x = 1 }\n",
"e027_method": "let mut x = 0\nstruct P { n: Int }\nimpl P { fn f(self) { x = 1 } }\n",
"e028": "import io { println }\nlet mut x = 0\nlet mut y = 0\nwatch x { y = x + 1 }\nwatch y { println(\"y={y}\") }\nact fn main() { x = 2 }\n",
"e029": "import io { println }\nlet mut x = 0\nwatch x { x += 1; println(\"seen={x}\") }\nact fn main() { x = 5; println(\"main={x}\") }\n",
"e030": "import collections { push }\nfn main() { let xs = [1]; push(xs, 2); () }\n",
"e031": "struct P { x: Int }\nfn f[T: Ord](x: T) -> T { x }\nfn main() { f(P { x: 1 }) }\n",
"e032": "import owner { x }\nact fn main() { x = 2 }\n",
"e033": "import owner { P, make }\nfn main() -> Int { make().hidden }\n",
"e034": "import fs { read_file }\nfn main() { read_file(\"x\") }\n",
"e035": "import io\nfn main() { println(\"hello\") }\n",
"e021_return": "fn f() -> Int { Some(1)? }\n",
"e021_infer": "fn f() { Some(1)? }\n",
"e021_kind": "fn f() -> Result[Int, String] { Ok(Some(1)?) }\n",
"e021_error": "fn f() -> Result[Int, String] { let x: Result[Int, Int] = Err(2); Ok(x?) }\n",
"e021_top": "let x = Some(1)?\n",
"e025": "struct S { me: S }\n",
"local_derived": "fn main() { let x = 1; derived y = x + 1 }\n",
"top_assignment": "let mut x = 0\nx = 1\n",
"local_source": "fn main() { let mut x = 1; let y = x * 2; x = 2; y }\n",
"generic_args": "fn id[T](x: T) -> T { x }\nfn main() { id::[Int](1) }\n",
"generic_unknown": "fn empty[T]() -> [T] { [] }\nfn main() { let xs = empty(); () }\n",
"enum_payload": "enum Tree { Leaf, Node(Tree, Tree) }\n",
"if_operand_chain": "fn f(x: Int) -> Int { let y = if x > 0 { 1 } else if x == 0 { 0 } else { -1 }; y }\n",
"if_operand_stmts": "fn f() -> Int { let y = if true { let x = 1; x } else { 2 }; y }\n",
"if_tail_ok": "fn f(x: Int) -> Int { if x > 0 { let n = 1; n } else if x == 0 { 0 } else { -1 } }\n",
"map_import": "fn main() { let m: Map[String, Int] = map_new(); m.len() }\n",
"map_empty": "fn main() { let m: Map[String, Int] = {}; m.len() }\n",
"map_key": "import collections { map_new }\nfn main() { let m: Map[Float, Int] = map_new(); m.len() }\n",
"live_mut_param": "live fn C(mut x: Int) { pub let n = x }\n",
"live_destructure": "live fn C() { let (a, b) = (1, 2) }\n",
"live_generic": "live fn C() { fn id[T](x: T) -> T { x } }\n",
"live_mut_fn": "live fn C() { fn set(mut x: Int) { x = 1 } }\n",
"live_derived_construct": "live fn C() { pub let n = 1 }\nderived c = C()\n",
"live_const_construct": "live fn C() { pub let n = 1 }\nconst C1 = C()\n",
"live_track_construct": "live fn C() { pub let n = 1 }\ntrack fn make() -> C { C() }\n",
"live_write": "live fn C() { pub let mut n = 1 }\nlet mut c = C()\nact fn main() { c.n = 2 }\n",
"live_private": "live fn C() { let n = 1 }\nlet c = C()\nact fn main() { let x = c.n; () }\n",
"live_function_value": "live fn C() { pub fn get() -> Int { 1 } }\nlet c = C()\nfn main() { let f = snap(c).get; () }\n",
"live_watch": "import io { println }\nlive fn C() { pub let n = 1 }\nlet c = C()\nwatch { println(\"{c.n}\") }\n",
"live_emit_index": "live fn C() { pub event Tick }\nlet cs = [C()]\nact fn main() { emit cs[0].Tick() }\n",
"live_cycle": "live fn C() { derived a: Int = b + 1; derived b: Int = a + 1 }\nlet c = C()\n",
"live_template_cross": "live fn C() { pub derived d: Int = total + 1 }\nlet c = C()\nderived total: Int = c.d\n",
"live_hold_cycle": "live fn C() { pub let mut children: [C] = []; pub event Adopt(x: C); on Adopt(x) { children.push(x) } }\nlet c = C()\nact fn main() { emit c.Adopt(c) }\n",
"live_dynamic_cycle": "live fn C() { pub let mut children: [C] = []; pub derived size: Int = 1 + children.map(|c| c.size).reduce(0, |a, b| a + b); pub event Adopt(x: C); on Adopt(x) { children.push(x) } }\nlet c = C()\nact fn main() { emit c.Adopt(c) }\n",
"live_drop": "live fn C() { let n = 1 }\nfn main() { C(); () }\n",
"live_fixed_param": "import io { println }\nlive fn C(x: Int) { pub fn get() -> Int { x }; pub act fn reset() { x = 2 } }\nlet mut c = C(1)\nact fn main() { c.reset(); println(\"{c.get()}\") }\n",
"live_plain_read": "live fn C() { pub let n = 1 }\nlet c = C()\nfn f(c: C) -> Int { c.n }\nfn main() -> Int { f(snap(c)) }\n",
"const_reactive": "let mut x = 1\nconst X = x\n",
"const_snap": "let mut x = 1\nconst X = snap(x)\n",
"closure_write": "fn main() { let mut x = 0; let f = |n: Int| match n { _ => { x = n; x } }; f(1) }\n",
"closure_return": "fn main() { let f = |n: Int| match n { _ => return n }; f(1) }\n",
"function_eq": "fn id(x: Int) -> Int { x }\nfn main() { id == id }\n",
"function_display": "import io { println }\nfn id(x: Int) -> Int { x }\nfn main() { println(\"{id}\") }\n",
"mut_expression": "fn grow(mut x: Int) -> Int { x += 1; x }\nfn main() { let mut x = 0; 1 + grow(mut x) }\n",
"mut_value": "fn grow(mut x: Int) { x += 1 }\nfn main() { let f = grow; () }\n",
"mut_missing": "fn grow(mut x: Int) { x += 1 }\nfn main() { let mut x = 0; grow(x) }\n",
"mut_method_result": "struct P { x: Int }\nimpl P { fn grow(mut self) -> Int { self.x += 1; self.x } }\n",
"mut_method_generic": "struct P { x: Int }\nimpl P { fn grow[T](mut self, n: T) { } }\n",
"method_mut_arg": "struct P { x: Int }\nimpl P { fn grow(self, mut n: Int) { n += 1 } }\n",
"generic_specialized_impl": "struct P[T] { x: T }\nimpl P[Int] { fn get(self) -> Int { self.x } }\n",
"eq_impl": "struct P { x: Int }\nimpl Eq for P { }\n",
"empty_array": "fn main() { let xs = []; () }\n",
"singleton_tuple": "fn main() { let x = (1,); () }\n",
"float_pattern": "fn f(x: Float) -> Int { match x { 1.0 => 1, _ => 0 } }\n",
"match_exhaustive": "fn f(x: Int) -> Int { match x { 0 => 1 } }\n",
"branch_widen": "fn f(x: Bool) { if x { 1 } else { 2.0 } }\n",
"semicolon": "fn f() { 1; }\nfn main() { let x: Unit = f(); () }\n",
"break": "fn main() { break }\n",
"match_watch_control": "let x = 1\nwatch x { match x { _ => { while false { }; () } } }\n",
"module_cycle": "import cycle\n",
"private_import": "import owner { secret }\n",
"missing_import": "import owner { missing }\n",
"alias_cycle": "type A = A\n",
"unicode_name": "fn main() { let café = 1; () }\n",
"main_args": "fn main(x: Int) -> Int { x }\n",
"extern_mut": "extern fn host(mut x: Int)\n",
"live_nested_type": "live fn C() { struct P { x: Int } }\n",
"extern_run": "extern fn host() -> Int\nfn main() { }\n",
"runtime_overflow": "fn main() { 9223372036854775807 + 1 }\n",
"runtime_index": "fn main() { [1][2] }\n",
"runtime_map_missing": "import collections { map_new }\nfn main() { let m: Map[String, Int] = map_new(); m[\"x\"] }\n",
"import_escape": "import ../../../../SPEC\n",
"duplicate": "const X = 1\nconst X = 2\n",
"unknown_type": "fn f(x: Mystery) { }\n",
"not_callable": "fn main() { let n = 1; n() }\n",
"method_missing": "fn main() { 1.missing() }\n",
"trait_mismatch": "trait T { fn f(self) -> Int; }\nstruct P { x: Int }\nimpl T for P { fn f(self) -> String { \"x\" } }\n",
"live_write_immutable": "live fn C() { pub let mut n = 1 }\nlet c = C()\nact fn main() { c.n = 2 }\n",
"loop_tracked": "import io { println }\nlet mut x = 2\ntrack fn sum() -> Int { let mut n = 0; for k in [x, x] { n += k }; n }\nderived y = sum()\nact fn main() { x = 3; println(\"{y}\") }\n",
"source_if_loop": "let mut x = 0\nif true { while false { x = 1 } }\n",
"if_stmt_derived": "fn f(x: Bool) -> Int { if x { 1 } else { 2 } }\nderived y = f(true)\n",
"ignore_effect": "import io { println }\nfn noisy() -> Int { println(\"x\"); 1 }\nfn ignore(f: fn() -> Int) -> Int { 1 }\nderived x = ignore(noisy)\n",
"unit_literal": "fn main() { () }\n",
"array_heterogeneous": "fn main() { [1, \"x\"] }\n",
"no_bit_assign": "fn main() { let mut x = 1; x &= 2 }\n",
"numeric_literal": "fn main() { 9223372036854775808 }\n",
"leading_minus": "fn main() {\n 1\n -1\n}\n",
"string_escape": 'fn main() { "\\q" }\n',
"match_watch_control2": "let x = 1\nwatch x { match x { _ => { while false { }; 1 } } }\n",
"branch_widen_check": "fn f(x: Bool) -> Int { if x { 1 } else { 2.0 } }\n",
"closure_default": "fn main() { let f = |i| i.price; f(1) }\n",
"live_dynamic_read_cycle": "live fn C() { pub derived d: Int = 1 + others.map(|c| c.d).reduce(0, |a, b| a + b) }\nlet mut others: [C] = []\nlet first = C()\nact fn main() { others = [first] }\n",
"live_uninit": "live fn C() { pub derived d: Int = total + 1 }\nlet all = [C()]\nfn sum(xs: [C]) -> Int { xs.map(|c| c.d).reduce(0, |a, b| a + b) }\nderived total: Int = sum(all)\nfn main() { }\n",
"e026": "rfn f() -> Int { 1 }\nact fn main() { f() }\n",
"reserved": "const __mod_mine = 1\n",
"runtime_cast": "fn main() { -1 as UInt8 }\n",
"runtime_divide": "fn main() { 1 / 0 }\n",
"event_arity": "event Tick(n: Int)\nfn main() { emit Tick() }\n",
"live_alias": "import io { println }\nlive fn C() { pub let mut n = 0; pub act fn inc() { n += 1 } }\nlet a = C()\nlet b = a\nact fn main() { b.inc(); println(\"{a.n} {a == b}\") }\n",
"live_constructor_input": "import io { println }\nlive fn C(x: Int) { pub let n = x }\nlet mut x = 1\nlet c = C(x)\nact fn main() { x = 2; println(\"{c.n}\") }\n",
"changing_method_expression": "fn main() { let mut xs = [1]; let x = xs.push(2); x }\n",
"method_import_control": "import collections { map_new }\nfn main() { let m: Map[String, Int] = map_new(); m.len() }\n",
"generic_type_args": "struct P[T] { x: T }\nfn f(x: P) { }\n",
"string_empty_interp": 'fn main() { "{}" }\n',
"live_alias_local": "import io { println }\nlive fn C() { pub let mut n = 0; pub act fn inc() { n += 1 } }\nfn main() { let a = C(); let b = a; b.inc(); println(\"{a.n} {a == b}\") }\n",
"enum_tuple_fix": "enum Tree { Leaf, Node((Tree, Tree)) }\n",
"if_operand_fix": "fn f(x: Int) -> Int { let y = if x > 0 { 1 } else { if x == 0 { 0 } else { -1 } }; y }\n",
"nesting_limit": "fn main() { " + '(' * 257 + '1' + ')' * 257 + ' }\n',
"unary_limit": "fn main() { " + '!' * 257 + 'true }\n',
"tree_limit": "fn main() { " + ' + '.join(['1'] * 1002) + ' }\n',
"e008_extern_const": "extern fn read() -> Int\nconst X = read()\n",
"e008_function_value": "act fn read() -> Int { 1 }\nfn apply(f: fn() -> Int) -> Int { f() }\nderived y = apply(read)\n",
"live_param_stale": "import io { println }\nlive fn C(x: Int) { pub derived double = x * 2; pub act fn reset() { x = 2 } }\nlet mut c = C(1)\nact fn main() { c.reset(); println(\"{c.double}\") }\n",
"library_value": "import math { sqrt }\nfn main() { let f = sqrt; f(4.0) }\n",
"mut_no_result": "fn grow(mut x: Int) { x += 1 }\nfn main() { let mut x = 0; let y = grow(mut x); y }\n",
"private_literal": "import owner { P }\nfn main() { P { hidden: 1 } }\n",
"ord_impl": "struct P { x: Int }\nimpl Ord for P { }\n",
}

def run():
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / 'owner.fl').write_text('pub let mut x = 0\npub struct P { hidden: Int }\npub fn make() -> P { P { hidden: 1 } }\nfn secret() { }\n')
    (OUT / 'cycle.fl').write_text('import module_cycle\n')
    shadow = OUT / 'shadow'
    shadow.mkdir(exist_ok=True)
    (shadow / 'io.fl').write_text('pub const LOCAL_ONLY = 1\n')
    # This support file was used only by the first exploratory pass.
    if (OUT / 'io.fl').exists(): (OUT / 'io.fl').unlink()
    result = {}
    runtime = {'e028','e029','local_source','live_hold_cycle','live_dynamic_cycle','live_plain_read','extern_run','runtime_overflow','runtime_index','runtime_map_missing','live_template_cross','live_fixed_param','loop_tracked','branch_widen','live_dynamic_read_cycle','live_uninit','runtime_cast','runtime_divide','live_alias','live_constructor_input','live_alias_local','live_param_stale'}
    for name, source in cases.items():
        source = source.replace('; ()', '; let done = 0')
        p = (shadow if name == 'e035' else OUT) / (name + '.fl')
        if name == 'e035' and (OUT / 'e035.fl').exists(): (OUT / 'e035.fl').unlink()
        p.write_text(source)
        args = [str(ROOT/'target/release/flux'), 'check']
        if name == 'e034': args += ['--target', 'web']
        args += [str(p)]
        r = subprocess.run(args, cwd=OUT, text=True, capture_output=True, timeout=5)
        output = re.sub(r'\x1b\[[0-9;]*m', '', r.stdout+r.stderr)
        record = {'source': source, 'command': ['flux']+args[1:], 'exit': r.returncode, 'check': output}
        if name in runtime:
            rr = subprocess.run([str(ROOT/'target/release/flux'), 'run', '--quiet', str(p)], cwd=OUT, text=True, capture_output=True, timeout=5)
            record.update(run_exit=rr.returncode, run=re.sub(r'\x1b\[[0-9;]*m', '', rr.stdout+rr.stderr))
        result[name] = record
    (OUT/'results.json').write_text(json.dumps(result, indent=2))
    for name, r in result.items():
        # Human-facing excerpts retain code, headline, and help; full output stays in JSON.
        lines = r['check'].splitlines()
        short = [x.strip() for x in lines if 'flux::' in x or re.match(r'\s*[x!⚠×]', x) or 'help:' in x or 'expected ' in x or 'found ' in x]
        print(name, 'exit='+str(r['exit']), '\n'+'\n'.join(short or lines[:5]))
        if 'run' in r: print('RUN',r['run'][-1400:])

if __name__ == '__main__': run()
