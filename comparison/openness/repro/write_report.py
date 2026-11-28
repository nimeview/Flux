"""Render the review from recorded, small release-binary probes."""
from pathlib import Path
import json, html, re, hashlib, subprocess

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
results = json.loads((HERE/'results.json').read_text())
rows = []

def add(group, key, title, why, verdict, relaxation, scores, comparison, also=(), note=''):
    rows.append(dict(group=group,key=key,title=title,why=why,verdict=verdict,relaxation=relaxation,scores=scores,comparison=comparison,also=also,note=note))

# N = necessary for the stated semantics; J = justified but relaxable;
# A = arbitrary relative to the advertised reactive guarantees, not inherently bad design.
add('Reactive reads, purity, and graph structure','e002','E002: a plain fn, including main, cannot directly read a module source/derived.',
    'S§Reactive semantics; R§Reading in functions: hidden reads could leave a cached result stale. Real concern, but the fn spelling is not necessary: instance-member reads in plain functions already work, and can be tracked when called by a derivation.',
    'J','Track ordinary reads dynamically when a derivation is active; otherwise read the current value. Keep explicit snapshots for deliberately untracked reads. Cost: implicit context sensitivity and effect inference.',(2,3),'READ',also=('live_plain_read',))
add('Reactive reads, purity, and graph structure','e003','E003: a derived binding cannot be assigned.',
    'S§Binding rules; R binding table; E: derived values have an equation, not independent storage. Allowing y = 4 while y = 2*x makes readers disagree about which definition wins until the next recomputation.',
    'N','Keep ordinary derived bindings read-only; an explicit writable lens could route writes to sources without changing this rule.',(2,2),'DERIVED')
add('Reactive reads, purity, and graph structure','e008_loop','Claimed E008: derived computations cannot loop, including through callees.',
    'S§Derived expressions explicitly includes callees; R says derived values cannot use loops. Thin: a bounded sum with local mutation is pure. The binary accepts this while loop and a reactive for-loop helper; the latter prints 6 after x changes from 2 to 3.',
    'A','Document pure loops as supported, retaining effect checks. Optional evaluation fuel addresses nontermination; recursion already has the same problem. Cost: documentation and, if desired, a fuel counter.',(0,1),'FLOW',also=('loop_tracked',),note='No E008 was emitted; the unused-binding warning is the complete relevant check result for the first probe.')
add('Reactive reads, purity, and graph structure','e008_return','Claimed E008: control flow reached through a derived callee.',
    'S§Derived expressions bans control flow, yet derived if/match are expressly supported. The binary accepts both explicit return and a statement if in a helper. Returning early does not make a pure computation impure.',
    'A','Define purity by effects, not return/if syntax; retain lexical return targets. Cost: clarify the contract; these examples already pass.',(0,1),'FLOW',also=('if_stmt_derived',))
add('Reactive reads, purity, and graph structure','e008_snap','E008: derived values cannot use snap, even through a helper.',
    'S§Derived expressions gives the real reason: a snap read is not followed. With derived y = snap(x)*2, changing x could leave y stale. R recommends snap for ordinary functions, but the error wrongly calls the read control flow and recommends contexts that do not themselves repair this dependency.',
    'N','Keep the ban on genuinely untracked reactive inputs to a derivation. A separate frozen-at-construction value could be explicit input, rather than a hidden read.',(1,2),'READ')
add('Reactive reads, purity, and graph structure','e008_action','E008: derived values cannot reach an act fn, including through a plain wrapper.',
    'S§Calling actions; R gives the concrete former 2 1 stale-value example. Actions intentionally do not track reads, so even a read-only action can hide x from the graph. This probe tests the transitive case.',
    'J','Permit an action proven to read no reactive state and have no effects, or add effect-polymorphic function types. Cost: interprocedural summaries; do not simply allow untracked reads.',(3,3),'PURE',also=('e008_function_value',))
add('Reactive reads, purity, and graph structure','e008_const','E008: even an effect-free, literal-returning act fn is forbidden in const.',
    'S§Const bindings; R states the ban but its stale-derived explanation does not justify rejecting a function returning 1 here. Const does not subscribe to anything. Purity and independence from mutable state are sufficient.',
    'J','Accept callees proved pure and independent of reactive state, regardless of act annotation. Cost: effect summaries and less annotation-based predictability.',(1,2),'PURE')
add('Reactive reads, purity, and graph structure','e008_track','E008: a track fn cannot reach an act fn, even if never called by a derived value.',
    'S§Calling actions; R: preserving transitive tracked reads is real. The unconditional declaration-level ban is conservative: this probe only returns 1, with no source reads or writes.',
    'J','Keep a tracked/pure signature by default, allow proven effect-free callees or context-sensitive checking. Cost: effect typing and possibly errors at use sites.',(2,3),'PURE')
add('Reactive reads, purity, and graph structure','e008_extern','E008: an extern fn cannot be reached from derived, const, or track fn.',
    'S§Host functions treats every host function as an action. Real default: a host clock or filesystem read can change without a source invalidation. But pure host math would be safe. The diagnostic calls extern an act fn and suggests track fn, which cannot preserve a host declaration.',
    'J','Add a host-validated pure extern contract, forbidding reactive reads/writes and nondeterminism. Cost: a trusted host boundary, like other FFI promises.',(0,1),'PURE',also=('e008_track_extern','e008_extern_const'))
add('Reactive reads, purity, and graph structure','e010','E010: cyclic equations in the reactive graph are rejected.',
    'S§Reactive semantics; R§Components; E: a = b+1 and b = a+1 has no solution; scheduling a glitch-free topological evaluation is impossible. The cycle path and suggested edge removal help, but neither book nor error develops the scheduling reason as fully as Rust teaches borrowing.',
    'N','Keep rejection for ordinary derived values. A separate delayed stream/fixed-point feature would need different semantics and convergence rules.',(2,2),'CYCLE')
add('Reactive reads, purity, and graph structure','e010_branch','E010 also rejects a cycle in an unreachable if branch.',
    'S combines acyclic templates with branch-sensitive runtime dependencies. The actual probe has a constant false branch, so its apparent cycle cannot execute. No bad execution exists for this example; conservative graph construction is the reason.',
    'J','Constant-fold branches before cycle analysis; more general dynamic cycles can be checked during evaluation. Cost: optimization-aware diagnostics or runtime errors instead of an unconditional static guarantee.',(1,1),'CYCLE')
add('Reactive reads, purity, and graph structure','e017_derived','E017: derived evaluation cannot print or perform I/O through a callee.',
    'S§Derived expressions; R purity example; E: memoized/lazy recomputation may run zero or several times. Logging or reading files there makes observable behavior depend on cache/scheduling choices instead of source changes.',
    'N','Keep the transitive effect prohibition. Put effects in watches/handlers, and feed external input into sources.',(2,2),'PURE')
add('Reactive reads, purity, and graph structure','e017_emit','E017: a derived computation cannot emit an event.',
    'S§Derived expressions; R explicitly includes emit. An event handler could write the source currently being derived, e.g. y=f(x) emits Inc which increments x. Recomputing changes its own inputs and can invalidate the current evaluation.',
    'N','Keep emit outside derived evaluation. No implicit deferred-event escape is compatible with ordinary pure equations.',(2,2),'PURE')
add('Reactive reads, purity, and graph structure','e017_const','E017: const initializers must be pure, although they run once at startup.',
    'S§Const bindings says startup evaluation, while S§Module items and R call const compile-time fixed. Reading a file once is deterministic within that run, but not a pure constant across runs. Purity supports the advertised constant meaning, not glitch freedom by itself.',
    'J','Keep pure const; teach the existing one-shot source initializer (let with no reactive reads) for effectful configuration. Cost: clearer startup-order documentation; an explicit init spelling could improve discoverability.',(2,2),'PURE')
add('Reactive reads, purity, and graph structure','const_reactive','E008: const cannot read reactive state, even an immutable module let.',
    'S const is independent of reactive inputs; R tells users to use const for fixed values. A frozen value derived from mutable x would stop following x, so banning hidden dependencies is coherent. The actual message refers to a derived expression instead of const.',
    'J','Allow immutable-source constants proven independent of mutable state, or a named startup snapshot form. Cost: constant propagation / a separate snapshot contract.',(0,1),'READ',also=('const_snap',))
add('Reactive reads, purity, and graph structure','ignore_effect','E017 conservatively rejects passing an effectful function to a callee that never calls it.',
    'S says passing a function to a call counts; only holding one is exempt. Here ignore(noisy) returns 1 and never prints. Purity checking conservatively treats every function argument as executable (types/check/stmt.rs, PureCheck::expr), rather than proving invocation.',
    'J','Summarize which callback parameters a function invokes; permit unused/stored callbacks. Cost: higher-order effect analysis, especially for escaping values.',(1,0),'PURE')

add('State writes, watches, and declaration placement','e011_watch','E011: watch cannot be declared inside an ordinary function.',
    'S§Reactive semantics; R first sentence: graph declarations are at file level (and live fn bodies). A repeatedly called factory containing watch would need clear ownership, disposal, capture, and initial-run semantics. The book states the placement rule without teaching those reasons.',
    'J','Allow scoped watches returning a disposable handle, or sugar for a live fn instance. Cost: lifecycle API and dynamic registration order.',(2,1),'SCOPE')
add('State writes, watches, and declaration placement','e011_on','E011: event handlers cannot be declared inside a function.',
    'S same placement rule; R uses module/component handlers. A handler created per call could multiply deliveries and retain locals indefinitely. Static module placement is one solution, not a requirement of deterministic batching.',
    'J','Allow scoped subscriptions with explicit ownership and disposal, and deterministic creation order. Cost: dynamic handler lifetime rules.',(2,1),'SCOPE')
add('State writes, watches, and declaration placement','local_derived','Reactive declarations are not local function variables; local derived syntax is unavailable.',
    'S§Module items and R: module/component values form the graph; fn let is local. Here derived is parsed as an undefined name rather than diagnosed as an invalid declaration. A local reactive cell needs a lifetime; a local ordinary value does not.',
    'J','Permit an explicit scoped reactive declaration with ownership, or direct users to live fn. Cost: scoped graph nodes; add a targeted error now.',(0,2),'SCOPE')
add('State writes, watches, and declaration placement','local_source','The same let spelling is a reactive source/derived at module level and an ordinary snapshot local inside fn.',
    'S§Binding rules; B and R explain the difference explicitly. This local y stays 2 when x becomes 2; it is not recomputed to 4. At module level, even let limit=10 is a source, triggering E002 in main. Convenience of automatic graph building is the reason, not a safety necessity.',
    'J','Add explicit source/derived forms and a migration path for plain module constants; retain inference as optional sugar. Cost: compatibility and one extra concept.',(0,3),'READ',also=('e002',),note='This is a semantic distinction, so successful checking is expected; no diagnostic warns that let changes meaning.')
add('State writes, watches, and declaration placement','top_assignment','A bare top-level assignment is a parse error, although top-level if may assign during initialization.',
    'S§Binding rules bans x=1; S§Modules and initialization permits writes inside a top-level if. if true { x=1 } has the same ordering issue, so the bare-form ban protects no additional graph invariant.',
    'A','Allow initialization statements in declaration order, settling at the documented module boundary. Cost: grammar and initialization ordering tests.',(0,1),'FLOW')
add('State writes, watches, and declaration placement','e016','E016: a module let without mut cannot be reassigned.',
    'S§Binding rules; R table; E: explicit mut distinguishes owned input state from fixed input and equations. This is a coherent mutability contract, independent of reactivity. Removing it silently turns fixed configuration into changeable state.',
    'N','Keep explicit mutability for ordinary bindings.',(2,2),'IMM')
add('State writes, watches, and declaration placement','e009_local','E009: an immutable local cannot be assigned.',
    'S§Binding rules; B; E: a stable binding can be copied/captured without tracking subsequent mutations. This is standard explicit-mutability discipline, not a special reactive restriction.',
    'N','Keep; declare let mut for a changing local.',(2,2),'IMM')
add('State writes, watches, and declaration placement','e009_const','E009: const cannot be assigned.',
    'S§Const; R table: an assignable constant contradicts the frozen-value contract. The error incorrectly calls this an immutable local; its let mut suggestion changes const into a reactive source and can introduce E002 in readers.',
    'N','Keep const immutable; improve the diagnostic to distinguish a constant from a local.',(1,2),'IMM')
add('State writes, watches, and declaration placement','e027','E027: fn main or another plain fn cannot directly change a source; write act fn.',
    'S§Who changes sources; R action example: caller-visible batching is real. Without the outer action, x=1; y=2 could expose an intermediate pair to watches. Explicit act is one way to mark the batch, not the only safe implementation.',
    'J','Infer source-writing functions as actions, still forbid them from derived contexts, and expose their effect in tooling/signatures. Cost: less explicit batch boundaries and effect inference.',(3,3),'WRITE')
add('State writes, watches, and declaration placement','e027_track','E027: track fn cannot directly change a source.',
    'S/R: tracked computations may run during derivation. f reads x then increments x would invalidate its own input. Separation of tracked computation and state transition is well-founded.',
    'N','Keep a pure tracked-computation role; move the transition into act fn/on. Renaming a track fn is appropriate only when its callers also permit an action.',(2,2),'PURE')
add('State writes, watches, and declaration placement','e027_method','E027: an impl method cannot directly write a module source, and cannot be declared act fn.',
    'S§Who changes sources; R says methods can call act fn. An act wrapper is already allowed, so requiring it adds indirection without an extra guarantee. A pure method must still be excluded from writes.',
    'J','Allow act methods, with the same source ownership and batching checks as act fn. Cost: method effect signatures and dispatch plumbing.',(2,2),'WRITE')
add('State writes, watches, and declaration placement','e028','E028 is a WARNING: a watch is allowed to write its own module source.',
    'S§Who changes sources and R explicitly allow it with a warning, despite S binding prose claiming narrower write locations. The required six-line probe checks successfully and runs: y=1, then y=3. No blanket watch-write prohibition exists. Writing another module or a derived/immutable value still hits E032/E003/E016.',
    'J','Keep a warning for redundant derived-state synchronization; allow intentional effect writes. Explain ordering and require an explicit action for larger transitions. Cost: no additional graph guarantee from simply moving the write into emit.',(2,2),'WRITE',note='The proposed emit workaround still settles in the SAME batch; it does not cure feedback cycles.')
add('State writes, watches, and declaration placement','e029','E029 warns about watch feedback; watches run at most once per batch.',
    'S§Writes and settling; R watch semantics; binary prints seen=1, main=5, seen=6. The final x change does not re-run its watch. The notification is lost, not the source write. A hard once-per-batch limit prevents infinite feedback but is not a fixed-point computation.',
    'J','Keep the warning; optionally provide an explicit next-batch event queue or bounded fixed-point mode. Cost: new timing/convergence semantics. Moving the same feedback to an on handler does not by itself break the cycle.',(2,2),'CYCLE')
add('State writes, watches, and declaration placement','e030','E030 warns when a functional collection result is dropped.',
    'S§Unused results; B/stdlib: push(xs,2) returns a copy while xs.push(2) changes a place. Warning is useful, but the exact hint xs = push(xs,v) is incomplete for this immutable xs: it also needs let mut. This is API consistency friction, not an arbitrary ban.',
    'J','Keep the warning, suggest let ys=push(xs,2) for immutable inputs or let mut plus xs.push(2). Cost: context-aware fixes.',(1,2),'VALUE')

add('Modules, visibility, targets, and ordinary static checking','e032','E032: an importing module cannot write another module\'s public mutable source.',
    'S§Modules; M counter example: owner-only mutation protects module invariants, e.g. balance and audit_total updated together. But a foreign write inside a batch would still propagate glitch-free; this is encapsulation, not required graph scheduling.',
    'J','Retain read-only exports by default; optional explicit exported setter/capability could authorize batched writes. Cost: a capability/visibility surface and weaker owner encapsulation.',(3,2),'OWNER')
add('Modules, visibility, targets, and ordinary static checking','e033','E033: other modules cannot name private struct fields, including literals, with, writes, and patterns.',
    'S§Fields; M§Private fields explains construction through methods. Direct construction of Account { balance:-1 } could violate its module invariant. Privacy is a conventional, coherent guarantee; unrelated to reactive graph safety.',
    'N','Keep default field privacy and explicit pub; no need to relax to reduce reactive friction.',(3,3),'OWNER',also=('private_literal',))
add('Modules, visibility, targets, and ordinary static checking','e034','E034: unavailable target modules/functions cannot be used (probe: fs on web).',
    'S§Targets; T/S status: web has no filesystem/process/env and is check-only. Missing host capabilities are real; the exact platform split is a deployment policy. A browser filesystem adapter is conceivable, but pretending the CLI API exists would be misleading.',
    'J','Allow an explicit host capability adapter. Cost: asynchronous/browser semantics and a real web runtime; keep E034 without such an adapter.',(2,2),'TARGET',note='Command is flux check --target web e034.fl; ordinary probes default to cli.')
add('Modules, visibility, targets, and ordinary static checking','e035','E035 warns that a local file with a library module name cannot be imported using that name.',
    'S stdlib resolution and M: library names win; local shadow/io.fl demonstrates it. Deterministic lookup is useful, but silently choosing a library instead of a local module is a namespace policy, not a reactive necessity. The warning gives a correct rename fix.',
    'J','Add an explicit local/package-qualified import namespace. Cost: one resolution form; retain the warning for ambiguous old imports.',(3,1),'MODULE')
add('Modules, visibility, targets, and ordinary static checking','e031','E031: a generic call must satisfy its declared trait bounds.',
    'S§Trait bounds; A: f[P] with P lacking Ord violates the declared interface. Static checking is necessary once a bound exists. In this exact f the parameter is unused, so the author can also remove an unnecessary bound. Bounds are optional; unbounded specializations are checked at use.',
    'N','Keep bounds checked at calls; improve missing-trait guidance to distinguish removable bounds from implementable traits.',(2,2),'TYPE')
add('Modules, visibility, targets, and ordinary static checking','e021_return','E021: ? needs a declared Result/Option return type and a matching operand kind.',
    'S§The ? operator; book errors section: propagating None into Int has nowhere to go. Result and Option have different failure values; automatic coercion would silently invent an error. The declared-type requirement is more conservative than the kind requirement.',
    'J','Infer a consistent Result/Option return when unambiguous; retain kind checking and use explicit match for conversion. Cost: bidirectional return inference and ambiguity diagnostics.',(2,2),'TRY',also=('e021_infer','e021_kind','e021_top'))
add('Modules, visibility, targets, and ordinary static checking','e021_error','E021: Result ? requires exactly the enclosing Result error type; no conversion.',
    'S§?; E: Err(Int) cannot inhabit Result[Int,String] unchanged. Exact equality is sufficient but not necessary: a deliberate conversion could preserve the propagation semantics. No reactive issue is involved.',
    'J','Add explicit map_error or a declared conversion trait analogous to Rust From; never guess a conversion. Cost: trait/conversion coherence and extra typing rules.',(2,2),'TRY')
add('Modules, visibility, targets, and ordinary static checking','e025','E025: a recursive type with no finite value is rejected.',
    'S§Recursive types; D: S must contain S forever; current eager values cannot construct it. Good diagnostic explains the base-case fix. Declaration rejection is stricter than necessary: an uninhabited type can be useful for impossible branches without ever constructing a value.',
    'J','Allow an explicitly uninhabited declaration while rejecting recursive construction; never silently build infinite eager values. Cost: exhaustiveness and representation support.',(3,2),'TYPE; Rust rejects direct infinite-size types but permits empty enums/indirect recursion; TS permits recursive declarations; Haskell permits recursive types/lazy values. JS reactive libraries add no such declaration ban.')
add('Modules, visibility, targets, and ordinary static checking','import_escape','E012: direct file imports cannot escape the project root.',
    'S§Modules/packages; M describes dependencies: declared package boundaries make resolution reproducible. ../shared could be a valid library, but should be a declared path dependency. The root is based on the nearest package or entry context, not a security sandbox for running Flux.',
    'J','Allow explicit configured import roots; keep dependency declarations preferred. Cost: reproducibility and tooling configuration.',(2,1),'MODULE')
add('Modules, visibility, targets, and ordinary static checking','module_cycle','E013: cyclic module imports are rejected.',
    'S§Modules; M states the rule. Eager mutable module initialization needs an order; a imports b imports a can read uninitialized state. This does not justify banning a type-only or pure-function-only cycle.',
    'J','Permit pure/type-only strongly connected components, reject cycles involving initialization state. Cost: staged module resolution and SCC analysis.',(2,1),'MODULE')
add('Modules, visibility, targets, and ordinary static checking','private_import','E022: a non-pub module item cannot be imported.',
    'S§Exports (one paragraph incorrectly labels private imports E014); M: explicit public APIs protect internal implementation. This is standard encapsulation; the diagnostic correctly says pub is required.',
    'N','Keep export privacy; correct the spec code cross-reference.',(2,2),'OWNER')
add('Modules, visibility, targets, and ordinary static checking','missing_import','E014/E001/E007: names, imports, and types must resolve in the current module.',
    'S§Modules; M: a module does not inherit its importer\'s scope. This avoids accidental name capture and gives separate files a stable meaning. Unknown exports cannot be linked. map_import below supplies the E001 case.',
    'N','Keep lexical/module resolution; add likely-import suggestions where known.',(2,2),'TYPE',also=('unknown_type',))
add('Modules, visibility, targets, and ordinary static checking','duplicate','E004: a scope cannot define the same item twice.',
    'S§Generics/events also disallows repeated parameters; B names and scopes. Two X definitions make bare X ambiguous. Overloading could be defined, but duplicate constants have no useful disambiguation here.',
    'N','Keep duplicate binding/parameter rejection; local shadowing remains a separate rule.',(1,1),'TYPE')
add('Modules, visibility, targets, and ordinary static checking','alias_cycle','E024: a type alias cannot recursively expand to itself.',
    'S§Type aliases; D/A: an alias is a synonym, so type A=A never yields a representation. A recursive struct/enum is the correct nominal boundary. This is distinct from recursive values or component handles.',
    'N','Keep aliases finite; use a nominal recursive type.',(3,1),'TYPE')
add('Modules, visibility, targets, and ordinary static checking','not_callable','E023: a value without a function type cannot be called.',
    'S§Function values; A: there is no call behavior for integer n. This is ordinary static typing, not reactive policy.',
    'N','Keep; an explicit callable trait would be a separate language feature.',(2,2),'TYPE')
add('Modules, visibility, targets, and ordinary static checking','method_missing','E019: a method must exist for the receiver type.',
    'S§Methods; A: direct static dispatch needs an implementation. The hint mentions impl Trait although inherent impl also works. The restriction is conventional; dynamic duck typing would change the language\'s checking model.',
    'N','Keep method resolution; mention both inherent and trait impl fixes.',(1,2),'TYPE')
add('Modules, visibility, targets, and ordinary static checking','trait_mismatch','E020: trait impl signatures must match the declared trait.',
    'S§Methods; A: returning String where callers expect Int breaks the interface. Necessary ordinary type safety. The error identifies the exact disagreement, but offers no repair or conceptual explanation.',
    'N','Keep trait signature checking.',(2,2),'TYPE')

add('Components and handle semantics','live_mut_param','A live fn parameter cannot be declared mut.',
    'S§Components; R: an instance keeps its own fixed copy. A copied parameter could safely be mutable instance-local state; copy semantics alone does not justify immutability. Explicit source members supply the supported alternative.',
    'J','Desugar mut parameters into per-instance source members, or explain using let mut n=x. Cost: clarify visibility and tracking of parameter reads.',(1,1),'LIFE')
add('Components and handle semantics','live_fixed_param','Claimed rule: component parameters stay fixed for the instance lifetime.',
    'S/R say fixed and nonreactive; actual check succeeds and reset changes x from 1 to 2. This violates the lifetime promise. A second probe reads derived double after the reset and prints 4, so these tests do NOT establish stale derived output. Source lowering marks parameter reads as constants; this deserves a contract audit.',
    'N','Enforce assignment rejection for fixed parameters, or explicitly promote mutable parameters to reactive source members. The former needs no new semantics.',(0,2),'IMM',also=('live_param_stale',),note='First runtime output is 2; second is 4. Both use let mut c to avoid unrelated receiver-mutability advice.')
add('Components and handle semantics','live_destructure','A component member cannot use a destructuring pattern.',
    'S§Declaration; R states it. let (a,b)=(1,2) can be lowered to a hidden tuple plus named projections; module-level destructuring already exists. No glitch or purity failure follows from doing the same per instance.',
    'A','Lower member destructuring to hidden members/projections, evaluate the initializer once. Cost: names, spans, and projection dependencies.',(2,1),'SYNTAX')
add('Components and handle semantics','live_generic','A component member function cannot have type parameters.',
    'S/R state the restriction, without a safety argument. A pure id[T] method on an instance can be monomorphized like a normal generic method. No unsafe program is demonstrated by lifting this restriction.',
    'A','Specialize member functions by instance-template and type arguments. Cost: code generation/cache keys, not a reactive semantic concession.',(1,1),'GENERIC')
add('Components and handle semantics','live_mut_fn','A component member function cannot have mut parameters or mut self.',
    'S/R state it; live member functions are already tied to an instance. Copy/write-back of an unrelated mutable argument is implementable, while mut self needs a meaningful handle-vs-value definition. The blanket parameter ban is stronger than the handle requirement.',
    'J','Allow ordinary mut arguments with normal write-back rules; retain no mut self until handle semantics are specified. Cost: lowering and effect propagation.',(1,1),'MUT')
add('Components and handle semantics','live_derived_construct','E008: derived/const/track fn cannot construct an instance.',
    'S§Construction; R: construction creates identity, handlers, and watches; recomputation could allocate a new subscribed instance each time. This is a real effect, not a pure value constructor. The offered track fn C fix is invalid for live fn.',
    'N','Keep allocation/subscription creation out of pure computations. Reconciliation/keyed construction would be an explicit owned effect feature.',(0,2),'PURE',also=('live_const_construct','live_track_construct'))
add('Components and handle semantics','live_constructor_input','A module let whose initializer constructs a component runs once, even when arguments are reactive.',
    'S§Construction; R gives Label(a,"a"). This probe prints 1 after x becomes 2: c.n holds the original parameter. Reconstructing on every input change would repeatedly replace identity and subscriptions. The one-time action semantics are real, but automatic let classification needs a visible warning in tooling.',
    'J','Add explicit construction/snapshot syntax or a compiler hover showing startup-only inputs; live parameters could instead be explicit reactive inputs. Cost: another binding mode/API.',(0,2),'LIFE')
add('Components and handle semantics','live_write','E032: code outside an instance cannot directly change its public source member.',
    'S§Member access; R: public read access is not write authority. An instance can preserve n>=0 through its own actions. This protects ownership, while batches alone could safely propagate external writes. With immutable c the binary emits E016 first, suggesting a change that then reveals E032.',
    'J','Support explicitly exported write capabilities/setters; keep read-only public handles by default. Cost: expanded API authority. Report E032 before irrelevant receiver-mutability advice.',(3,2),'OWNER',also=('live_write_immutable',))
add('Components and handle semantics','live_private','E033: non-pub component members are inaccessible even elsewhere in the same file.',
    'S§Member access; R: member privacy belongs to the component interface. The actual message incorrectly says private to its module, although the probe is in that module; its live fn pub help is correct.',
    'N','Keep component privacy; use an instance-specific diagnostic instead of module wording.',(1,2),'OWNER')
add('Components and handle semantics','live_function_value','Component functions/events cannot be read as first-class values.',
    'S§Member access; R states the restriction. A bound function must retain the instance, and its action/tracking effect must survive indirect calls. That needs representation, but is not incompatible with purity or deterministic ordering.',
    'J','Introduce bound member closures/event handles carrying instance ownership and effects. Cost: lifetime and effect analysis; a lambda calling the member is a limited workaround.',(1,1),'FUNCTION')
add('Components and handle semantics','live_watch','A targetless watch cannot read an instance member.',
    'S§Member reads; R gives the exact derived-name workaround. Runtime tracking already supports member reads in derived values and track functions, so the limitation is in the watch\'s static name-based subscription model. No inherent reactive failure is shown.',
    'J','Track instance-member reads dynamically in targetless watches, or accept an expression target. Cost: subscriptions must switch/dispose as handles change.',(2,2),'SCOPE')
add('Components and handle semantics','live_emit_index','emit requires a name chain; an indexed instance must be bound first.',
    'S§Member access; R explicitly explains let item=list.items[i]. Evaluation of cs[0] once followed by event delivery is unambiguous. Current diagnostics expose SymbolId and invent a method failure on [Int64], giving no useful repair.',
    'A','Accept any once-evaluated instance expression as an emit receiver. Cost: expression lowering and preserving evaluation order.',(0,2),'SYNTAX')
add('Components and handle semantics','live_cycle','E010: a component template cannot have cyclic bare-member derived equations.',
    'S§Cycles; R: the probe a=b+1,b=a+1 has no fixed point, as for module equations. Naming C.a/C.b in the cycle report is useful.',
    'N','Keep, with any future delayed/fixed-point graphs explicitly separate.',(2,2),'CYCLE')
add('Components and handle semantics','live_template_cross','The component/module template dependency graph must also be acyclic.',
    'S§Cycles; R states cross-component/module E010. The binary check passes, but run rejects C.d → total → C.d before execution. Current driver source explicitly attempts IR validation in Check phase, so source and supplied-binary behavior must not be conflated.',
    'J','Retain rejection of an actual value cycle; conservative type/template-level cycles could be allowed with per-instance runtime cycle checks. Cost: later errors and dynamic graph validation.',(0,2),'CYCLE',note='Score 0 is for check failing to report this known-invalid program; run\'s E010 message merits 2.')
add('Components and handle semantics','live_dynamic_read_cycle','A derived value may not come to depend on itself through dynamic instance reads.',
    'S§Cycles; R: a source array is updated to contain the instance whose d reads that array. Returning an old d would violate freshness; unconditional recursion would diverge. This is a runtime failure rather than a check-time restriction.',
    'N','Keep the runtime cycle guard; improve the trace with the instance dependency path.',(0,2),'CYCLE',note='Check correctly says ok because the value-dependent cycle is not fully statically decidable; runtime explanation quality is 2.')
add('Components and handle semantics','live_hold_cycle','An instance may not hold itself, even when no reactive dependency cycle exists.',
    'S§Holding instances; R states the rule; compiler test comment explains the missing rationale: reference counting could never release the cycle, so its global handlers could keep reacting. This probe has no derived members: it is a lifetime/retention rule, distinct from E010.',
    'J','Provide weak instance links, or a cycle collector/owner-arena with explicit disposal. Cost: weak-link handling or collection overhead and disposal scheduling. The book should teach the retention mechanism.',(0,1),'LIFE',note='Check ok; run rejects at on Adopt. Runtime message says what, but gives neither a weak-link option nor a repair (score 1).')
add('Components and handle semantics','live_uninit','An instance may not read a module binding before its initial value is set.',
    'S§Cycles and initialization; R: construction eagerly evaluates the member while total is still waiting on construction. A placeholder/old value would be a glitch. The runtime trace identifies C.d and module initialization.',
    'N','Keep the guard; possible future two-phase initialization may defer some member evaluation without exposing unset values.',(0,2),'CYCLE',note='Check ok; run reports E018. Runtime score 2; static score 0 is not a claim this dynamic failure must be statically decidable.')
add('Components and handle semantics','live_drop','E030 warns when a constructed instance is immediately discarded.',
    'S§Lifetime; R: handlers/watches do not keep it alive. A bare C() otherwise looks like permanent registration but will be disposed by the batch end. Warning explains both lifetime and binding repair.',
    'N','Keep this warning and handle-based lifetime rule; scoped explicit ownership is preferable to invisible permanent registration.',(3,3),'LIFE')
add('Components and handle semantics','live_alias_local','Copying an instance handle aliases identity; it does not copy the state.',
    'S§Types and handles; R: local let b=a, b.inc() prints 1 true for a.n and a==b. This differs from arrays/structs\' value-copy behavior. The module-level alias probe unexpectedly fails with an internal Cell<C> receiver, so the documented unrestricted copy story has a binary gap.',
    'J','Keep handle identity; offer explicit clone-state construction if desired. Fix module alias receiver unwrapping. Cost: an explicit cloning API, not implicit duplicated subscriptions.',(0,3),'LIFE',also=('live_alias',))
add('Components and handle semantics','live_nested_type','A live fn body cannot declare types, nested components, imports, or const members.',
    'S§Declaration enumerates members; R does too. Types/imports can live outside the template; a constant member could be a nonreactive immutable per-instance value. No scheduling reason is supplied for the whole syntactic exclusion.',
    'A','Allow lexically scoped types/constants where their lifetime is clear; hoist template-independent items. Cost: scoping and export resolution.',(0,1),'SYNTAX')

add('Generics, values, grammar, and library surface','generic_args','Explicit type arguments at generic call sites are not supported.',
    'S§Types; A explicitly says f::[T]() is unavailable, with contextual annotation as workaround. Inference is ergonomic when it works, but explicit arguments cannot break a graph or purity. Parser lists unrelated grammar expectations.',
    'A','Add explicit generic call/type-constructor arguments, reusing existing monomorphization. Cost: grammar disambiguation and diagnostics.',(0,2),'GENERIC')
add('Generics, values, grammar, and library surface','generic_unknown','A generic call with unconstrained type arguments is rejected; bare generic type names also need all arguments.',
    'S§Inference; A: empty() has infinitely many [T] interpretations; P without T has no complete static type. The explicit-type-call ban makes the annotation workaround mandatory. The call diagnostic has unusually useful contextual help.',
    'N','Keep unresolved types an error; add the explicit argument escape hatch above rather than arbitrary default types.',(2,3),'GENERIC',also=('generic_type_args',))
add('Generics, values, grammar, and library surface','enum_payload','A tuple enum variant has exactly one payload value; Node(Tree,Tree) is rejected.',
    'S§Types; D demonstrates one payload/named fields, but does not adequately highlight this Rust/Elm/Haskell surprise. Node((Tree,Tree)) already checks, proving the restriction is representational sugar, not a reactive necessity. The parser offers named-field/struct fixes but omits the simplest tuple fix.',
    'A','Desugar multi-payload variants to one tuple payload; preserve pattern arity in syntax. Cost: parser/pattern sugar and compatibility decisions for display.',(2,1),'SYNTAX',also=('enum_tuple_fix',))
add('Generics, values, grammar, and library surface','if_operand_chain','An if used as an operand has no direct else if chain.',
    'S§if/match as operands describes a smaller expression grammar; B teaches statement if. Tail-statement else if checks, and else { if ... } also checks as an operand. There is no semantic safety difference.',
    'A','Share if grammar in expression and statement positions. Cost: parser/AST normalization; preserve existing branch value/effect checking.',(0,0),'FLOW',also=('if_tail_ok','if_operand_fix'))
add('Generics, values, grammar, and library surface','if_operand_stmts','Operand-if branches cannot contain statements, unlike a function-tail if.',
    'S distinguishes statement block values from operand expressions, but the book does not warn clearly. let y=if true {let x=1;x}else{2} is pure and well-defined; moving the same if to tail position works. The restriction follows the parser, not reactivity.',
    'A','Use value blocks in both positions; apply context-sensitive purity checks to effects. Cost: one coherent block AST instead of separate forms.',(0,0),'FLOW')
add('Generics, values, grammar, and library surface','match_watch_control','Match arm blocks in derived/watch/on contexts cannot contain loops/control-flow, emit, or module assignments.',
    'S§Block match arms; D: non-function arms have an expression-only subset. For derived values, effect rejection is real; for a watch/on, moving an allowed loop/write one level into a match arm changes no guarantee. This is a placement restriction over a lowering subset.',
    'J','Allow full arm blocks in effect contexts, keep derived purity checks, and preserve return/break lexical scopes. Cost: uniform lowering of match bodies.',(1,1),'FLOW')
add('Generics, values, grammar, and library surface','map_import','map_new needs an import, while map methods such as m.len do not.',
    'S§Standard library; D/stdlib explain imported free functions and built-in methods. This is consistent namespace policy, but constructor discoverability is poor: E001 gives no import help. Explicit imports do not protect reactive semantics.',
    'J','Suggest import collections { map_new }; optionally add Map::new(). Cost: symbol-to-import lookup or one type-function alias.',(1,2),'MODULE',also=('method_import_control',))
add('Generics, values, grammar, and library surface','map_empty','There is no empty map literal ({} is not a Map initializer).',
    'S§Maps says map_new plus annotation; D uses it. Deterministic key order does not require verbose construction. {} could be disambiguated by the expected Map type or a dedicated map literal syntax.',
    'A','Add a typed empty map literal or Map::new(). Cost: grammar/context disambiguation; no graph cost.',(0,1),'MAP')
add('Generics, values, grammar, and library surface','map_key','Map keys are only integers, String, Bool, or Char; not tuples, structs, or floats.',
    'S§Maps says sorted deterministic order; D covers maps. Rejecting arbitrary unordered/function keys is justified; rejecting (Int,Int) is unnecessary because lexicographic order is deterministic. Float NaN needs an explicit total-order policy.',
    'J','Support structurally ordered tuple/enum/struct keys, and optional explicit total float keys. Cost: comparison definitions and compatible display/iteration order.',(2,1),'MAP')
add('Generics, values, grammar, and library surface','empty_array','An unannotated empty-array binding cannot infer its element type.',
    'S§Constructor types; D: [] supplies no element evidence. Annotation repairs the ambiguity. Unlike functions such as empty(), this error embeds its repair in expected-type text rather than help.',
    'N','Keep ambiguity rejection or infer from later uses where sound. Cost of the latter: delayed local inference.',(2,2),'GENERIC')
add('Generics, values, grammar, and library surface','closure_default','An unconstrained closure parameter defaults to Int, so field reads need context or an annotation.',
    'S§Closures; A explicitly documents this and gives |i:Item|i.price. Defaulting to Int is a historical policy, not necessary for soundness. The actual diagnostic explains the default and gives a right repair.',
    'J','Delay inference until uses, or report an unknown parameter type rather than guessing Int. Cost: broader constraint solving and potential compatibility changes.',(3,3),'GENERIC')
add('Generics, values, grammar, and library surface','closure_write','Closures capture locals by value and cannot assign a captured mutable local.',
    'S§Closures; A illustrates snapshots. x was declared mut in the outer function, so the generic immutable-local message\'s let mut fix is wrong here. Copy capture is coherent, but mutable closure-owned captures can also preserve value semantics.',
    'J','Offer explicit mutable captures with closure state or reference-cell types; retain value capture by default. Cost: callable mutability/lifetime and effect rules.',(0,2),'FUNCTION')
add('Generics, values, grammar, and library surface','closure_return','return is forbidden in a closure body, including its match arms.',
    'S§Function values; A says the body\'s value is its result. That is an explanation of expression syntax, not a safety argument: an early return targeting the closure is unambiguous. Rust/JS support this without compromising outer control flow.',
    'A','Permit return to the nearest closure/function boundary. Cost: control-flow lowering; continue rejecting jumps across closure boundaries.',(1,1),'FUNCTION')
add('Generics, values, grammar, and library surface','function_eq','Functions and values containing functions have no ==, !=, or contains comparison.',
    'S§Equality; A explains the transitive exclusion. Extensional equality is undecidable, while code-address equality gives surprising results for closures. Rejecting ambiguous equality is justified. However a reactive function-valued derived node also needs a documented change-detection policy.',
    'J','Keep no extensional equality; optionally expose explicit function identity, never silently equate functions by behavior. Cost: stable closure identity and clarified graph change detection.',(1,2),'FUNCTION')
add('Generics, values, grammar, and library surface','function_display','Functions and any containing value cannot be printed/interpolated/to_string.',
    'S§Function values and interpolation; A: no text form is defined. Unlike equality, a placeholder such as <fn> is trivial and safe; banning diagnostic display of a record merely because it contains a callback is an avoidable usability cost.',
    'A','Give functions a non-executable opaque debug display and recursively display containers. Cost: formatting stability; do not expose addresses as portable identities.',(2,2),'FUNCTION')
add('Generics, values, grammar, and library surface','mut_expression','A call with mut arguments must be a statement or a let initializer, not a larger expression.',
    'S§mut parameters; B: calls copy parameters in and write final values back. 1+grow(mut x) has a perfectly definable left-to-right write-back point. The restriction simplifies lowering, not graph correctness.',
    'J','Sequence call, write-back, then surrounding expression; preserve evaluate-once places and failure atomicity. Cost: temporary values and ordering specification.',(2,2),'MUT')
add('Generics, values, grammar, and library surface','mut_missing','Each mut parameter requires explicit mut on a mutable-place argument.',
    'S§mut parameters; B gives precise rules. Writing back to a temporary or immutable binding loses a result/violates mutability. Explicit mut at the call makes copy-back visible, though it is a design choice rather than a reactive necessity.',
    'J','Retain writable-place checking; optional inference of call-site mut with tooling indication. Cost: mutation becomes less visually explicit.',(2,2),'MUT')
add('Generics, values, grammar, and library surface','mut_value','Functions with mut parameters cannot be stored or passed as function values.',
    'S§mut parameters; B/A explain direct-call workaround. Current fn(T)->U type cannot describe write-back places, so erasing the effect would be unsafe. A richer function type could describe it.',
    'J','Add first-class mut-parameter function types, or require an explicit adapter closure. Cost: indirect call ABI/place validation and effect-aware typing.',(2,2),'MUT')
add('Generics, values, grammar, and library surface','mut_method_result','A mut self method must be a standalone place call and cannot return a value.',
    'S§Methods; A states copy-back and no result. Ordinary mut-parameter functions can return values, so there is no general semantic reason methods cannot return (changed self,result) internally. Transactional copy-back can still occur before yielding the result.',
    'J','Allow results and expression calls after copy-back. Cost: the same sequencing representation already needed by mut functions.',(1,2),'MUT')
add('Generics, values, grammar, and library surface','mut_method_generic','A method with its own type parameters cannot have mut self.',
    'S§Methods; A states it without a causal rationale. A generic method writing a value derived from T is no less pure locally than a monomorphic one. This is a specialization/lowering limitation.',
    'A','Specialize before applying ordinary mut-self lowering. Cost: combining two existing compiler paths.',(1,1),'GENERIC')
add('Generics, values, grammar, and library surface','method_mut_arg','Methods cannot have mut parameters other than mut self.',
    'S§Methods; B/A state the limit. The free-function form can already modify several arguments; a method receiver plus mutable argument is a routine equivalent. No graph failure distinguishes them.',
    'A','Apply the free-function mut ABI to methods, with evaluate-once receiver/arguments. Cost: lowering and alias/write-back ordering rules.',(1,1),'MUT')
add('Generics, values, grammar, and library surface','generic_specialized_impl','A generic type cannot have an impl specialized to one concrete argument.',
    'S§Methods; A: only impl[T] P[T] is accepted. A disjoint P[Int] method is safe. Overlap requires a policy, but does not justify rejecting all concrete impls.',
    'J','Allow disjoint concrete impls, reject overlapping methods unless precedence is explicit. Cost: coherence/overlap checking.',(2,1),'GENERIC')
add('Generics, values, grammar, and library surface','eq_impl','Built-in Eq and Ord cannot be implemented for a user type.',
    'S§Trait bounds; A: Eq is structural comparability and Ord only numbers/String; a same-named user trait is separate. Built-in meaning must remain consistent, but structurally ordered records or declared lawful comparators are possible. The ban is broader than deterministic graph comparison requires.',
    'J','Keep graph change equality structural by default; add explicit ordering/comparison traits or structural Ord for eligible types. Cost: law/coherence policy; do not let arbitrary effectful equality control settling.',(2,1),'TYPE',also=('ord_impl',))
add('Generics, values, grammar, and library surface','singleton_tuple','Tuples have two or more values; (x,) is forbidden.',
    'S§Tuples; D: grouping is (x). A singleton tuple is distinguishable by its comma and useful to generic APIs; forbidding it protects no reactive property.',
    'A','Accept one-element tuples with trailing comma. Cost: type/display/pattern support for arity one.',(2,1),'SYNTAX')
add('Generics, values, grammar, and library surface','unit_literal','Unit is a type/block result, but the obvious () expression is rejected.',
    'S uses () as the value of non-value blocks and lists Unit; no clear book warning says there is no literal. A no-op fn returning Unit can supply it, so this is grammar friction rather than a semantic impossibility.',
    'A','Add the () literal. Cost: parser and constant representation already present in IR.',(0,0),'SYNTAX')
add('Generics, values, grammar, and library surface','float_pattern','Float literal patterns are rejected, even with a catch-all.',
    'S§Literal patterns gives the rationale: exact float equality is a trap; D describes the allowed literals. But x==1.0 is allowed and 1.0 is exactly representable. A ban on only pattern syntax is paternalistic rather than necessary for correctness.',
    'J','Allow literal patterns with IEEE equality, warn for NaN/nonexact comparisons, or teach guarded matches. Cost: matching policy; no exhaustiveness credit for float literals.',(1,1),'PATTERN')
add('Generics, values, grammar, and library surface','match_exhaustive','A match must be exhaustive; numeric/string literal arms need a catch-all, guarded arms do not prove coverage.',
    'S§Patterns; D; E: input 1 in match x {0=>1} otherwise has no result. Requiring total coverage is real. The diagnostic only says catch-all rather than naming the uncovered domain; book examples are more helpful.',
    'N','Keep exhaustiveness. Better finite missing-case examples would improve diagnostics without weakening it.',(1,2),'PATTERN')
add('Generics, values, grammar, and library surface','branch_widen_check','Branch/return types must agree without implicit numeric widening in annotated functions.',
    'S§Block value; B explains branch values. A function declared Int cannot return Float unchanged. Literal-directed coercion could be well-defined, but implicit Float-to-Int would lose information. An unannotated control probe passes; that does not establish this annotated program is valid.',
    'J','Allow only lossless, explicit or expected-type-directed numeric literal conversions; keep incompatible branches rejected. Cost: conversion rules and clearer inferred-type reporting.',(1,2),'TYPE',also=('branch_widen',))
add('Generics, values, grammar, and library surface','array_heterogeneous','Arrays must have a single element type.',
    'S§Types; D first sentence. [1,"x"] has no single element representation/type under Flux\'s current rules. Tagged unions could represent it, but need an explicit sum type or a new union-type feature.',
    'N','Keep homogeneous arrays; model alternatives with enums.',(1,2),'TYPE')
add('Generics, values, grammar, and library surface','semicolon','A final semicolon does not discard a block value as it does in Rust.',
    'S§Separators/Block value; B: semicolon is only a separator. f(){1;} returns Int, so using f as Unit fails. This rule is internally coherent, but a Rust programmer gets no explanation from the type mismatch.',
    'J','Either keep and teach the difference clearly, or add explicit discard expression syntax. Cost of Rust-style semicolons: a breaking grammar/value change.',(1,1),'FLOW')
add('Generics, values, grammar, and library surface','break','break/continue require a loop in the same function; they cannot cross closure boundaries.',
    'S§Statements; B gives the rule and return alternative. Jumping out of an unrelated caller\'s loop has no stable lexical target and would break ordinary call semantics.',
    'N','Keep lexical loop boundaries.',(2,3),'FLOW')
add('Generics, values, grammar, and library surface','no_bit_assign','Bitwise compound assignment forms such as &= are absent.',
    'S§Operator precedence explicitly lists the missing forms; B documents only arithmetic compound assignment. x=x&2 is already legal. This is a small language-surface omission with no reactive justification.',
    'A','Desugar bit compound assignment using the existing evaluate-once place machinery. Cost: parser/operator coverage.',(0,1),'SYNTAX')
add('Generics, values, grammar, and library surface','unicode_name','Identifiers/code outside strings/comments are ASCII-only.',
    'S§Lexical structure names the ASCII grammar; book barely discusses it. Avoiding confusables is a reasonable simplicity choice, but café can be an unambiguous name and Unicode string data is already supported.',
    'J','Support normalized Unicode XID identifiers with optional confusable lint. Cost: normalization/versioned identifier policy.',(2,0),'SYNTAX')
add('Generics, values, grammar, and library surface','leading_minus','A line starting with binary minus is rejected instead of continuing the prior expression.',
    'S§Statement boundaries; E teaches it. This prevents f() followed by -1 silently becoming f()-1. The diagnostic unusually explains the ambiguity and both repairs.',
    'J','Keep for now; a newline-sensitive expression grammar could accept standalone unary minus. Cost: clear continuation/statement boundary rules and possible compatibility changes.',(3,2),'SYNTAX')
add('Generics, values, grammar, and library surface','string_escape','Only documented escapes are valid; bare {} is not a formatting placeholder.',
    'S§Lexical structure/Interpolation; B/E: braces contain an expression; literal braces require escapes. Silently accepting unknown escapes would conceal typos. Supporting more deliberate escapes is a surface feature, not a reactive constraint.',
    'J','Keep unknown-escape errors; add useful explicit Unicode/hex escapes if needed, plus a targeted empty-interpolation repair. Cost: lexical support.',(2,2),'SYNTAX',also=('string_empty_interp',))
add('Generics, values, grammar, and library surface','reserved','Claimed rule: names beginning __mod_ are reserved for compiler-generated names.',
    'S§Modules states E000; book does not foreground it. The binary accepts const __mod_mine=1. Source type_scope reserves some reference/declaration paths, so coverage is incomplete. Name hygiene is real, but banning a user prefix is an implementation strategy.',
    'J','Use symbol identity/hygienic namespaces internally; until then consistently reject the reserved prefix with a specific diagnostic. Cost: internal name representation or complete validation.',(0,0),'SYNTAX')
add('Generics, values, grammar, and library surface','e026','E026 warns for older syntax; it does not prohibit it.',
    'S§Diagnostics/tooling; book tools: rfn still compiles and warns to use track fn; module-dot paths and old handler payload syntax also warn. A uniform modern spelling is reasonable, but not a reactive safety rule.',
    'J','Keep migration warnings configurable and offer concrete edits; avoid reporting them as hard restrictions.',(2,2),'SYNTAX')

add('Execution, host boundaries, and resource limits','main_args','main must take no parameters and return Unit or Int; Int is an exit status.',
    'S§Entry point; getting-started/tools: the CLI has a fixed entry ABI. A parameter has no supplied value. Wider entry signatures could be supported, but need an argument-passing convention.',
    'J','Optionally allow main(args:[String]) and Result exits with an explicit CLI mapping. Cost: ABI and exit/error policy; keep the current signature check until then.',(2,2),'TARGET')
add('Execution, host boundaries, and resource limits','extern_mut','extern fn parameters cannot be mut.',
    'S§Host functions; R/status/stdlib host section: host receives values, with no copy-back ABI. A mutable place cannot be reconstructed from an ordinary host argument; this is a real ABI limitation, not a necessary language-wide ban.',
    'J','Return an explicit updated value or define host copy-back results. Cost: a new trusted host ABI and failure atomicity rules.',(1,1),'MUT')
add('Execution, host boundaries, and resource limits','extern_run','The CLI cannot run a program declaring any extern fn without a host implementation, even if unused.',
    'S§Host functions states check accepts but run does not provide host functions. Required symbols are validated before startup; this is deterministic linking. An unused extern could be dead-stripped without affecting pure graph behavior.',
    'J','Link only reachable extern declarations, or let the CLI load an explicit host adapter. Cost: reachability / host plugin loading.',(0,2),'TARGET',note='Check ok is expected; run E018 identifies the missing host symbol and declaration (runtime score 2).')
add('Execution, host boundaries, and resource limits','event_arity','Events/handlers have fixed payload arity and types; emit does not invent missing values.',
    'S§Events; R examples. Tick(n:Int) needs one Int. Missing a value otherwise makes on Tick(n) undefined. This is ordinary static function-like arity checking.',
    'N','Keep arity/type validation; defaulted payload arguments could be a separate explicit feature.',(1,2),'TYPE')
add('Execution, host boundaries, and resource limits','numeric_literal','Integer literals must fit their selected width; unsuffixed integers are Int64.',
    'S§Numbers/Numeric semantics; B: a too-large literal cannot represent its advertised value. This example can use a u64 suffix instead. Rejection is necessary for fixed-width exact literals; a targeted suffix help would reduce friction.',
    'N','Keep range checking; suggest the wider type/suffix where it fits.',(2,2),'LIMIT')
add('Execution, host boundaries, and resource limits','runtime_overflow','Integer arithmetic, division by zero, casts, and shifts fail on invalid values instead of wrapping silently.',
    'S§Numeric semantics; B checked methods; book errors: this prevents silent corruption. Runtime failure still leaves earlier batch writes in place; deterministic batching is not transactional rollback. check accepts these runtime-value operations.',
    'J','Keep checked defaults; explicit checked/wrapping/saturating alternatives can serve numeric code without weakening defaults. Cost: API surface and exact width semantics.',(0,2),'LIMIT',also=('runtime_divide','runtime_cast'),note='Runtime messages identify integer overflow / division by zero / numeric cast out of range and a source frame (runtime score 2).')
add('Execution, host boundaries, and resource limits','runtime_index','Indexing outside an array or a missing map key fails at runtime; use get for Option.',
    'S§Indexing/Maps; D: no valid value exists at the requested place. get is the supported nonthrowing alternative. These are bounds checks, unrelated to arbitrary reactive policy; runtime errors cannot currently be caught inside Flux.',
    'N','Keep safe bounds/key checks; keep get and potentially add checked place-update APIs.',(0,2),'LIMIT',also=('runtime_map_missing',),note='Check ok is expected; runtime says index out of bounds / key not found with a frame (runtime score 2).')
add('Execution, host boundaries, and resource limits','nesting_limit','Parser resource caps: 256 bracket levels, 256 consecutive unary operators, 1000 AST depth.',
    'S§Limits says these avoid native stack aborts; stdlib book lists bracket depth. Three sub-5 KB syntax-only probes verify each actual diagnostic. Resource limits are justified; these exact numbers are engineering choices, not graph mathematics.',
    'J','Keep graceful errors; permit configured compiler budgets or iterative parsing where practical. Cost: resource predictability/implementation effort.',(3,1),'LIMIT',also=('unary_limit','tree_limit'),note='Source is linked rather than expanded into hundreds of repeated tokens in this table.')

add('Generics, values, grammar, and library surface','library_value','Library free functions cannot be used as first-class values without a wrapper closure.',
    'S describes user function values; the checker explicitly distinguishes library builtins. sqrt itself is rejected even though |x|sqrt(x) can supply the same typed pure function. No reactive safety property distinguishes the two; current builtins lack the ordinary callable representation.',
    'J','Reify typed builtin function pointers/adapters, preserving their effect summaries. Cost: first-class builtin dispatch and polymorphic builtin specialization.',(2,0),'FUNCTION')
add('Generics, values, grammar, and library surface','mut_no_result','A mut-parameter function with no declared result has no bindable value, even when its body ends in an expression.',
    'S§mut parameters; B states it. The compiler currently returns changed arguments through an internal copy-back ABI, so an explicit result annotation distinguishes user result from that protocol. Ordinary fn return inference already exists; its absence here is not a graph guarantee.',
    'J','Infer a user result separately from changed arguments, or offer a targeted annotation fix. Cost: result/copy-back ABI typing; keep no-result calls unbindable while they truly return nothing.',(2,2),'MUT')

def code(s):
    return '<code>'+html.escape(s.strip()).replace('|','&#124;').replace('\n','<br>')+'</code>'

def excerpts(raw, omit_unused=True):
    if raw.strip() == 'ok': return 'ok'
    blocks = re.split(r'(?=flux::E\d{3})', raw)
    out=[]
    for block in blocks:
        if not block.strip(): continue
        if omit_unused and block.startswith('flux::E015'): continue
        ls=block.splitlines()
        if ls and ls[0].startswith('flux::E'): out.append(ls[0].strip())
        for line in ls:
            if re.match(r'\s*x ',line): out.append(re.sub(r'^\s*x ', '', line))
            elif re.match(r'\s*\| ',line): out.append(re.sub(r'^\s*\| ?', '',line))
            elif 'help:' in line: out.append(line.strip())
            elif line.startswith('error: runtime error:'): out.append(line)
    if not out and 'flux::E015' in raw:
        return excerpts(raw,False)
    return '\n'.join(out) or raw.strip()

def render_row(n,r):
    keys=(r['key'],)+tuple(r['also'])
    src=[]; diag=[]
    for i,k in enumerate(keys):
        rr=results[k]
        path='repro/'+('shadow/' if k=='e035' else '')+k+'.fl'
        src.append(f'[{k}]({path})'+ ('<br>'+code(rr['source']) if i==0 and k!='nesting_limit' else ' (complete program)'))
        ex=excerpts(rr['check'])
        diag.append(f'**{k}: check exit {rr["exit"]}**<br>'+code(ex))
        if 'run' in rr:
            diag.append(f'run exit {rr["run_exit"]}: '+code(excerpts(rr['run'])))
    assessment=f'{r["why"]}<br>**{ {"N":"Necessary","J":"Justified but relaxable","A":"Arbitrary"}[r["verdict"]]}.** {r["relaxation"]}'
    if r['note']: assessment+='<br>'+r['note']
    cells=[f'**{n}. {r["title"]}**<br>'+'<br><br>'.join(src), '<br><br>'.join(diag), assessment, f'**{r["scores"][0]} / {r["scores"][1]}**<br>{r["comparison"]}']
    return '| '+ ' | '.join(cell.replace('|','&#124;') for cell in cells)+' |\n'

HEADER = '''**Flux 0.4.0 restriction review — independent assessment**

The reactive core is defensible; the claim that all restrictions follow from that core is not. Pure equations, tracked reads, acyclic evaluation, and explicit state transitions are coherent guarantees. Several blanket placement rules, generic/mut-method exclusions, and grammar asymmetries merely reflect current implementation choices. The book explains the action/derived distinction substantially better than most of these edges. Diagnostics are often actionable, but some recommend invalid fixes; this is not consistently Rust-level explanation.

**Evidence and scoring.** Reviewed `SPEC.md`, the book, HIR lowering, type/effect checks, and IR dependency collection. Executed only small sequential `target/release/flux check FILE.fl` and selected `run --quiet FILE.fl` probes; no builds, tests, JIT runs, or benchmarks. All created files are under this directory. Complete sources, command arguments, exit statuses, and untrimmed output are in [results.json](repro/results.json); [audit.py](repro/audit.py) reproduces the probes. The repeated-token parser probes are below 5 KB and execute no Flux code. Limits requiring large allocations or many specializations were deliberately not exercised.

The release binary is the behavioral evidence, not an assumption that the source and executable correspond exactly. In particular, current [driver source](../../flux_driver/src/pipeline.rs) explicitly runs IR validation in Check phase, yet the supplied executable accepts a cross-template cycle in `check` and rejects it in `run`. This review does not claim that rebuilding current source would reproduce every binary discrepancy; rebuilding was prohibited.

Each numbered row is a restriction or a documented semantic trap. A linked file is the complete minimal reproducer; module support files are given below. Diagnostics are literal excerpts with source-frame decoration and irrelevant E015 warnings removed. `ok`/exit 0 really means no rejection; warnings are not described as bans. A few documented restrictions are absent or violated in the supplied binary, which is explicitly noted. These are targeted observations, not proofs of whole-language soundness.

Scores are **message / book**, each 0–3: **0** absent, misleading, or wrong repair; **1** names the rule with little usable explanation; **2** correct rule and useful repair, but weak mechanism/why; **3** explains what, why, and an applicable fix. For runtime-only checks the static message score is 0 because `check` emits none; the runtime message is scored separately in the row. Successful semantic controls also score 0 for diagnostic discoverability, not compiler incorrectness. Book scores assess the explanatory material, not agreement with an unverified claim. No average conflates these categories.

**Necessary** means required by the stated contract of that construct (including ordinary typing/privacy), not a theorem that every language must make that choice. **Justified but relaxable** means a coherent policy with a concrete safe extension. **Arbitrary** means no demonstrated connection to the advertised guarantees; it may still be a legitimate scope or simplicity choice. Where no real bad execution exists for the rejected program, the review says so rather than inventing a reactive rationale.

Local references: **S** = [SPEC](../../SPEC.md), especially §§2–5 and 7–11; **R** = [reactive](../../docs/flux/src/reactive.md); **M** = [modules](../../docs/flux/src/modules.md); **A** = [abstraction](../../docs/flux/src/abstraction.md); **D** = [data](../../docs/flux/src/data.md); **B** = [basics](../../docs/flux/src/basics.md); **E** = [reading errors](../../docs/flux/src/reading-errors.md); **T** = [tools](../../docs/flux/src/tools.md). These references identify the stated rationale; the counterfactual and verdict are the reviewer's assessment. Main implementation anchors: [HIR statement/write checks](../../flux_hir/src/lower/stmts.rs), [component lowering](../../flux_hir/src/lower/live.rs), [type purity checks](../../flux_types/src/check/stmt.rs), [effect propagation](../../flux_types/src/check/effects.rs), [generic bounds](../../flux_types/src/mono.rs), [IR read collection](../../flux_ir/src/lower/reads.rs), and [watch-cycle analysis](../../flux_hir/src/cycles.rs).

**Cross-system comparisons (the profile in every row is part of that row).** These compare equivalent operations, not identical Flux syntax. Rust/TypeScript are not reactive runtimes; ordinary memory safety/type checking does not imply Flux-style dependency freshness. Elm/Haskell do not expose ambient imperative reactive sources as ordinary pure values. “Static” means rejection by language/type checking; “runtime” is a guard/warning after code executes; “allow” is neither a blanket purity check nor a promise that bad feedback will converge. JS libraries inherit JavaScript/TypeScript syntax and type restrictions for nonreactive operations.

Context7 was used for Solid, Vue, and MobX; primary docs were read for the other systems. Version sensitivity matters: current Solid docs describe an owned-scope write error, whereas [Solid 1.9.5 signal implementation](https://raw.githubusercontent.com/solidjs/solid/v1.9.5/packages/solid/src/reactive/signal.ts) does not statically prevent memo setters. Do not turn “memos should be pure” into a universal compile-time claim. [Current Solid diagnostic](https://docs.solidjs.com/reference/solid-js/diagnostics/reactive-write-in-owned-scope/) describes runtime detection of writes during owned computation. Vue calls computed purity a [best practice](https://vuejs.org/guide/essentials/computed.html), not a general static effect system. MobX's [enforceActions](https://mobx.js.org/configuration.html) is configurable runtime discipline; the [implementation](https://raw.githubusercontent.com/mobxjs/mobx/main/packages/mobx/src/core/derivation.ts) uses development console warnings, not a guarantee that a forbidden write was rolled back. MobX [actions are untracked](https://mobx.js.org/actions.html). Svelte 5 has runtime [state_unsafe_mutation and cycle errors](https://svelte.dev/docs/svelte/runtime-errors); its [derived documentation](https://svelte.dev/docs/svelte/$derived) also permits temporary derived overrides since 5.25. None of these libraries statically bans all arbitrary I/O in computations.

The nonreactive baselines are Rust's [const evaluation](https://doc.rust-lang.org/reference/const_eval.html), [privacy](https://doc.rust-lang.org/reference/visibility-and-privacy.html), and [try propagation](https://doc.rust-lang.org/reference/expressions/operator-expr.html#the-try-propagation-expression); TypeScript's [function/constraint checking](https://www.typescriptlang.org/docs/handbook/2/functions.html), [generics](https://www.typescriptlang.org/docs/handbook/2/generics.html), and [modules](https://www.typescriptlang.org/docs/handbook/2/modules.html); Elm's [Cmd/Sub effect boundary](https://guide.elm-lang.org/effects/); and Haskell's [pure expressions](https://www.haskell.org/onlinereport/haskell2010/haskellch3.html), [data/class declarations](https://www.haskell.org/onlinereport/haskell2010/haskellch4.html), [modules](https://www.haskell.org/onlinereport/haskell2010/haskellch5.html), and [IO](https://www.haskell.org/onlinereport/haskell2010/haskellch7.html). These are semantic comparisons, not results of running those compilers in this study.

| Profile | Rust | TypeScript | Elm | Haskell | SolidJS | Vue | MobX | Svelte 5 |
|---|---|---|---|---|---|---|---|---|
| READ | Safe global/shared reads allowed; no reactive graph | Ambient reads allowed | Pure reads allowed; ambient mutable state unavailable statically | Pure reads allowed; IORef reads require IO statically | Ordinary synchronous function reads tracked in computations; untrack allowed | Same dynamic tracking; explicit untracked reads possible | Ordinary reads tracked; action reads untracked | Function reads tracked in derived; untrack allowed |
| PURE | Ordinary fn may do I/O; const restricts effects statically | Effects allowed in any function | Pure computation cannot execute external effects; Cmd interpreted by runtime | IO effects cannot execute as pure results statically, barring unsafe escapes | Purity contract; current owned-scope state writes runtime error; no general I/O ban | Computed purity guidance; arbitrary effects not statically prevented | Computed purity guidance/configured runtime write warnings; not a static effect type | Derived state writes runtime error; arbitrary console/I/O not generally statically rejected |
| DERIVED | Immutable bindings statically protected; interior mutation is explicit | const/readonly statically protect binding/property | Values immutable; static | Values immutable; static | Memo accessor has no setter; API/types | Readonly computed write warns at runtime; TS readonly; explicit setter variant | Getter-only computed cannot be assigned; explicit computed setter allowed | let-derived can be temporarily overridden since 5.25; const cannot |
| IMM | let needs mut; const cannot be reassigned; static | const/readonly assignment static errors; object contents may mutate | No imperative reassignment; static | Same; mutations require effectful references | JS binding rules; signal writes via setter | JS bindings; ref.value mutable unless readonly | JS bindings/observable properties, configurable action warning | JS bindings; state writes generally allowed outside derivation |
| WRITE | Writes allowed with mutable authority; no action keyword | Writes allowed; no action keyword | Update returns a new Model; no imperative source writes | State/IO actions express mutation; static effect distinction | Effects/event callbacks may write; loops are programmer/runtime concern | Watch callbacks may write; feedback may fail at runtime | enforceActions warns at runtime when configured; actions batch | Effects/handlers may write; runaway feedback gives runtime depth error |
| CYCLE | Recursion allowed; cyclic const evaluation rejected; no reactive DAG check | Cyclic objects/functions allowed; divergence possible | Recursive functions allowed; not a general reactive DAG language | Recursive data/bindings allowed; bottom/divergence possible | Dynamic feedback can loop/fail; no whole-program static DAG proof | Watch feedback allowed, scheduler recursion guard; no static DAG proof | Runtime computed-cycle / reaction-convergence checks; no static DAG proof | Runtime derived-self/depth errors; not Flux's global acyclic-template rule |
| SCOPE | Scoped subscriptions can be expressed with owned closures/Drop | Callback registration allowed in functions | Sub values assembled in pure functions; runtime owns subscriptions | Subscriptions can be managed inside IO | Owned roots/computations can be created in functions; disposal responsibility | watch in functions allowed; ownership/unwatch controls lifetime | autorun/reaction anywhere; disposer controls lifetime | Effects require active owner or explicit effect root; runtime orphan error |
| OWNER | Field/module privacy static; pub mutable access needs appropriate authority | Module export/private/readonly static; exported object fields can be mutable | Hidden constructors/opaque modules; immutable data | Hidden constructors/exports; IORefs mutable with authority | Signals/stores have no mandatory declaring-module-only writer rule | Props readonly convention/runtime warnings; stores/refs can be shared | Shared observables allowed; actions not module ownership | Props ownership/binding checks/warnings; explicit bindable API; no Flux module-source rule |
| FLOW | Expression blocks, else-if, loops, early returns allowed; lexical jump checks | Statement if/loops allowed; expression alternative is ternary/IIFE; return lexical | Expression if/case and recursion; no imperative loops/return | Expression if/case/let, recursion/monadic loops; no imperative early return | JS/TS flow allowed inside reactive callbacks | Same | Same | Same in derived.by/effects; derived must remain state-write-free |
| GENERIC | Explicit arguments, inferred arguments, generic methods; static constraints | Explicit/inferred arguments, generic methods; static constraints | Inferred parametric types; no Rust-style explicit call type arguments or user typeclasses | Inferred polymorphism; GHC type applications are an extension; static class constraints | Host TS inference/arguments; no extra reactive ban | Same | Same | Same |
| TYPE | Static type/arity/trait/exhaustiveness checks; nominal types and coercion rules | Static structural typing/constraints; no default exhaustive switch requirement | Static types, homogeneous lists, exhaustive cases; no user typeclasses | Static types/classes; pattern completeness usually warning/runtime failure, configurable | Host JS dynamic or TS static typing | Same | Same | Same; additional template/compiler checks |
| FUNCTION | Mutable captures and early return allowed; no closure extensional Eq; debug display restricted by traits | Mutable captures/return allowed; function reference equality and display allowed | No mutable captures; functions lack general comparable equality; debug facilities differ | No mutable captures; functions lack standard Eq/Show; monadic state possible | Host function values allowed; effects guarded when invoked, not merely carried | Same, without full purity enforcement | Same; explicit actions retain runtime semantics | Same; state writes checked during derived evaluation |
| MUT | &mut parameters/methods can return, be generic, and occur in expressions; borrow rules static | Object/reference mutation allowed; no Flux copy-back ABI | No imperative mut parameters | State/IO reference operations, no Flux copy-back ABI | Host JS mutation/TS calls; no mut-method generic prohibition | Same | Same, action policies apply to observable writes | Same, derived write guard applies |
| LIFE | Owned values move/clone; Rc handles alias; Rc cycles allowed and can leak | Object references alias; GC permits cycles | Immutable values, no ordinary mutable handle graph | Lazy/cyclic immutable structures; GC; IO references can cycle | Identity/disposal owned by reactive roots, not Flux's handle DAG | Component/effect scopes and GC; no universal handle DAG ban | References/GC plus disposer; cycles not universally forbidden | Component/effect ownership and GC; ordinary cyclic objects allowed |
| MAP | BTreeMap orders Ord keys incl tuples; HashMap alternative; constructors imported | Map constructor accepts arbitrary key identity; empty new Map; object literal alternative | Dict uses comparable ordered keys incl supported tuples; Dict.empty | Data.Map uses Ord keys; Map.empty; tuples have Ord instances | JS Map/object rules; collections need explicit reactive wrapping | Reactive Map supported; JS key rules | Observable Map supported; JS key rules | SvelteMap/objects; JS key rules; reactive Map class imported |
| PATTERN | Exhaustive match static; float patterns allowed with caveats/lints | Switch/branches allowed; exhaustiveness usually manual/never pattern | Case exhaustiveness static; pattern grammar differs | Incomplete patterns normally warnings; float numeric patterns permitted | Host JS/TS branches, no Flux match grammar | Same | Same | Same; markup each/if syntax is separate |
| MODULE | Explicit use/modules; cycles of item references possible; dependency graph rules separate | Imports explicit; ESM cycles allowed but initialization TDZ may fail at runtime | Imports/exports explicit; cyclic modules rejected | Explicit imports/exports; report permits recursive modules, GHC needs source/signature machinery | Host import resolution; no built-in rule that io.fl is unimportable | Same | Same | Same |
| TRY | ? propagates compatible residual; Result error conversion via From; static | No equivalent ? propagation operator; throw/Result helpers allowed | Maybe/Result.andThen or case; static kinds | Maybe/Either monadic composition; error types must fit | Host JS/TS error mechanisms; reactive library adds no Flux-style ? | Same | Same | Same |
| TARGET | Target cfg/capabilities and link errors; ABI must match | libs/types/host availability; runtime missing APIs possible | Platform/kernel/ports boundary; cannot invent arbitrary host I/O | FFI/link/platform availability checked by compiler/linker/runtime | Browser/SSR differences; unavailable host APIs can fail | Same | Same | Browser/SSR restrictions, compile/runtime depending API |
| VALUE | Value/reference semantics explicit; unused must_use may warn | Arrays/objects shared; functional results can be discarded, optional lint | Persistent values; unused return has no mutation | Persistent values; unused pure result has no mutation | Host methods/API semantics; no universal E030 | Same | Same | Same |
| SYNTAX | Multi-payload enums, singleton tuples, (), Unicode identifiers supported | Equivalent tagged objects/tuples, undefined/void, Unicode names; own grammar | Multi-argument constructors; tuples arity 2/3 and (); language-specific grammar | Multi-argument constructors, tuples incl unit (no ordinary unary tuple); Unicode names | Host JS/TS syntax; no extra reactive justification for Flux parser bans | Same | Same | Host JS/TS plus Svelte syntax; not Flux's grammar |
| LIMIT | Fixed-width overflow/casts/bounds have explicit differing checked/wrapping/panic policies | Number arithmetic IEEE; no universal fixed-width overflow check; indexing can be undefined | Safe array access returns Maybe; finite runtime/resources remain | Integer unbounded, Int bounded; partial indexing may fail; resources finite | Host JS numeric/index behavior and runtime budgets | Same | Same | Same |

Profiles are intentionally reusable, not claims that every system has the same construct. For example, FLOW says Rust permits statements in an expression-if, TS uses another expression form, and Elm/Haskell use pure value expressions. Generic-call absence in Elm is not evidence that Flux needs it to enforce reactivity. Rust's allowance of [Rc reference cycles](https://doc.rust-lang.org/book/ch15-06-reference-cycles.html) is the useful contrast for Flux's instance-holding prohibition: memory safety does not inherently require leak freedom. Vue's [watch lifecycle/disposal rules](https://vuejs.org/guide/essentials/watchers.html) illustrate a concrete alternative to module-only subscriptions.

**Module repro support.** [owner.fl](repro/owner.fl) contains `pub let mut x=0`, a public `P` with private `hidden`, public `make()`, and private `secret()`. [cycle.fl](repro/cycle.fl) imports `module_cycle`, whose repro imports `cycle`. E035 is isolated in [shadow/e035.fl](repro/shadow/e035.fl) beside [shadow/io.fl](repro/shadow/io.fl), containing only `pub const LOCAL_ONLY=1`. No other probe has a shadowing io.fl. All checks use the repro directory as working directory; commands in results.json include absolute paths.
'''

TAIL = '''
**Remaining explicit platform and budget limits.** These are part of the inventory, but deliberately not forced with expensive programs. S§Limits specifies 10,000 active calls (including emit chains), 1,024 generic specializations / type depth 32, collections at most 10,000,000 elements, and built strings at most 100,000,000 bytes. The first three syntax limits above were measured; these larger limits were not. No fabricated “actual diagnostic” is supplied: the spec names `call depth limit exceeded`, E005 specialization errors, and `allocation limit exceeded`. Profile LIMIT applies. Verdict: justified but relaxable; budgets protect the process, while exact thresholds could be configurable with per-run memory/evaluation budgets. Cost: resource accounting and reproducibility. Message score unmeasured; book 1–2 (stdlib lists several limits and the spec explains process-abort prevention, but generic budgets are not comparably taught). A large probe would conflict with the benchmark-in-progress constraint.

The platform inventory also includes: web is check-only, WASM generation is a stub, maps/components and other unsupported JIT representations fall back to the VM, there is no registry/version dependency resolution, dependencies must be declared directly, and runtime errors cannot be caught inside the language. These are explicitly described in S§§8–11 and [Status and limits](../../docs/flux/src/status.md), not consequences of reactive purity. They are justified implementation/deployment limits, relaxable through backends, a registry/resolver, explicit transitive-dependency policy, and a Result boundary for recoverable failures. Catching a graph-settle failure would additionally need a precise partial-batch recovery policy. Profile TARGET/MODULE/LIMIT applies; book 2. No unsupported build/backend/registry commands were run. Do not infer from “JIT fallback” that a program is rejected, or compare fallback-heavy benchmarks as if everything were native.

**What the explanations get right, and where they fail.** The best passage is R's old action-read example: it shows the wrong result, explains untracked reads, and gives track fn as a correct repair. The component lifetime section is detailed about behavior; field privacy has good construction examples; E025 is unusually good about the base-case repair. E002, E027, and the watch warnings give practical guidance.

Several explanations confuse a rule with its reason. “Must be pure” does not explain why a bounded local loop is forbidden; “keeps a copy” does not entail an immutable parameter; “names one value” does not justify banning destructuring. A programmer should be told when a limit comes from lowering or the lack of an ABI feature. Stating that candidly would improve trust more than framing every omission as reactive safety.

The generic E008 help is particularly poor for snap, const reads, extern declarations, and live constructors. E009 suggests let mut for an already-mutable captured local, E016 can suggest mut for an instance write that will remain illegal, and E030's assignment repair can fail on an immutable array. Parser expectation lists for operand-if, explicit generic arguments, empty maps/Unit, and indexed emit reveal compiler grammar rather than explain the language. The main diagnostic table in S omits E035, despite the book listing it. S also calls private imports E014 in one paragraph but E022 in its table; source-writing introductory prose still understates act fn/top-level-if/watch behavior.

The observed fixed-parameter mutation and module-alias failures are more significant than a low wording score: they make the advertised mental model unreliable. The loop/control-flow discrepancy may be best repaired by loosening the contract to match already-safe functionality. Check/run disagreement on template cycles should be fixed or attributed to the exact supplied-binary/source mismatch before publishing benchmark-adjacent claims. The source already contains an apparent check-phase fix; this review did not rebuild to validate it.

**Ranked friction reductions that preserve pure derived values, glitch-free evaluation, and deterministic batches.**

1. **Make actual behavior and explanations agree.** Validate fixed parameters, module handle aliases, and check/run cycle parity against this exact binary fingerprint. Replace E008's generic help with construct-specific fixes and distinguish source vs local vs captured-variable writes. Cost: focused compiler/diagnostic work; no guarantee lost.
2. **Unify expression and statement value blocks.** Permit else-if and local statements inside operand-if, and full match arms in watch/on. Retain lexical jumps and transitive purity checks. Cost: parser/AST/lowering cleanup; eliminates several unrelated grammar traps.
3. **Make effect-free control flow legal and documented.** Pure loops, early return, and local mutation should be governed by the same effect rules as arithmetic; measured helper loops already work. If responsiveness matters, add an explicit evaluation budget rather than pretending loops imply impurity. Cost: contract correction and optional runtime fuel.
4. **Add explicit generic call arguments and payload sugar.** `id::[Int](1)` and `Node(a,b)` can lower to existing specialization and tuple-payload machinery. Also accept `()` and singleton tuples. Cost: syntax and pattern coverage; graph behavior unchanged.
5. **Complete action/mut method composition.** Allow act methods, generic mut-self methods, returned values from mut-self, and mut parameters on methods, with call/write-back sequencing before an enclosing expression continues. Cost: method effect signatures and a coherent copy-back ABI.
6. **Remove name-only restrictions on instance operations.** Allow once-evaluated expression receivers for emit, bound member functions/events, and dynamic member subscriptions in watches. Cost: owned handles, effect-aware closures, and subscription switching; retain deterministic creation/disposal order.
7. **Make collection construction and use discoverable.** Add Map::new or a typed empty literal, suggest imports in E001, and tailor E030 to immutable vs mutable bindings. Add ordered structural map keys. Cost: API/diagnostic polish and structural ordering support.
8. **Teach or expose state authority and graph lifetime explicitly.** Keep module/instance write authority by default, with optional exported setters/capabilities; offer scoped subscription sugar over live instances and weak handle links. Cost: APIs and lifecycle bookkeeping. No implicit permanent watchers or uncontrolled writes.
9. **Improve higher-order effect precision.** Do not reject ignore(noisy) when noisy is never invoked; permit proved pure state-independent actions/host functions in constant contexts. Cost: callback invocation/effect summaries and a trusted pure-host contract. Never waive the ban for unknown effects.
10. **Make batch feedback behavior impossible to misunderstand.** Say “notification suppressed after this watch already ran,” not that the source write is lost. Explain that emit joins the same batch. Offer an explicit next-batch queue only if its order is specified. Cost: documentation now, deliberate scheduling API later; do not silently change to an unbounded fixed-point loop.

**Restrictions that should stay exactly in substance.**

| Keep | Concrete reason |
|---|---|
| Derived values cannot perform I/O, emit, or source writes, including indirect calls | Recomputing an equation must not change inputs or duplicate externally visible effects according to cache/scheduler choices. |
| A derivation cannot secretly use untracked reactive reads (snap/untracked action reads) | Otherwise x can change while y retains an answer computed from old x. A future relaxation must restore tracking or make the frozen input explicit. |
| Actual derived-value dependency cycles must fail, statically where known and dynamically where necessary | a=b+1,b=a+1 has no solution; stale-value fallback would violate freshness. Conservative impossible edges may still be pruned. |
| Ordinary derived bindings remain read-only | Assignment cannot silently replace an equation; writable projections/optimistic overrides need separately named semantics. |
| Explicit immutable bindings and const stay immutable; fixed component parameters really stay fixed | Otherwise captures and untracked constant reads lose their advertised meaning. Mutable reactive parameters must be a different, tracked construct. |
| Type/arity checks, exhaustive value matches, alias termination, safe bounds/key checks | These prevent missing results, invalid representation, and reading nonexistent values; none are excuses to hide grammar limitations. |
| Existing private fields/members and read-only export defaults | They preserve declared abstraction boundaries. Explicit pub/setter/capability opt-ins are clearer than letting every observer mutate state. |
| Effects run after graph settling in a defined batch order, with failures stopping further work | Observers must not receive an inconsistent intermediate graph. Preserve the specified order and document partial writes; do not imply rollback. |
| Source-only state transitions remain distinguishable from pure tracked computation | Whether marked explicitly or inferred, writes must still open/join defined batches and be rejected from pure equations. |

Watch writes are **not** on a list of absolute prohibitions: Flux allows them. Nor should arbitrary parser restrictions be defended as if they were a borrow checker. The fair verdict is a principled reactive kernel, useful but uneven teaching, and a substantial layer of relaxable implementation restrictions whose causal explanations need to be made honest.
'''

def main():
    order=list(dict.fromkeys(r['group'] for r in rows))
    rows.sort(key=lambda r:order.index(r['group']))
    sha=hashlib.sha256((ROOT/'target/release/flux').read_bytes()).hexdigest()
    head=subprocess.run(['git','rev-parse','HEAD'],cwd=ROOT,text=True,capture_output=True,check=True).stdout.strip()
    meta={'date':'2026-11-27','source_head':head,'binary_sha256':sha,'spec_sha256':hashlib.sha256((ROOT/'SPEC.md').read_bytes()).hexdigest(),'check_probes':len(results),'run_probes':sum('run' in r for r in results.values()),'inventory_rows':len(rows)}
    (HERE/'review-metadata.json').write_text(json.dumps(meta,indent=2)+'\n')
    content=HEADER+f'\nAudit fingerprint: source HEAD `{head}`; release SHA-256 `{sha}`. **{len(results)} check probes, {meta["run_probes"]} tiny run probes, {len(rows)} inventory entries**; date 2026-11-27. This count includes accepted controls and subcases, not {len(results)} separate bans.\n'
    current=None
    for i,r in enumerate(rows,1):
        if r['group']!=current:
            current=r['group'];content+='\n**'+current+'**\n\n| Restriction and complete minimal program | Actual diagnostic (trimmed) | Stated rationale, counterfactual, verdict and concrete relaxation | Message / book; comparison profile |\n|---|---|---|---|\n'
        content+=render_row(i,r)
    content+=TAIL
    (HERE.parent/'restrictions.md').write_text(content)
    print(json.dumps(meta,indent=2))

if __name__=='__main__':main()
