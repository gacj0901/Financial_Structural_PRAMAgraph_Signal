# Structural stack authority

The Rust runtime keeps three boundaries explicit:

1. PRAMA Protokol `0.3.0` is consumed from the pinned Rust crate at commit
   `ddb91cad792fed3674aa81a5650fab6c187fc1a5` and is not copied.
2. D_O v9, ODCE v0.1 and K-MEM are causal ports of the reference implementations in
   `AtadynamiK-GIT/LLM-SVM_PRAMA-Protokol`.
3. The financial adapter is new, versioned domain code. It maps PRAMA Gamma rows into
   bounded geometry using strictly-prior robust references; it never consumes raw price
   or future outcomes.

## D_O v9

The port preserves the reference geometry window, minimum support, recurrence definition,
prior-transition ridge operator, residual coherence, effective-rank variation capacity,
contraction, alert delay, hysteresis grace and transport/mobility state machine. Ridge
training ends at `t-1`; appending future rows cannot change a prior D_O result.

The raw financial geometry coordinates are:

```text
squash(delta_tilde)
clip(xi / theta)
clip((lambda - lambda_min) / (lambda_max - lambda_min))
0.5 + 0.5 tanh(M / theta)
0.5 + 0.5 tanh(G)
squash(A)
```

`lambda_min` and `lambda_max` come from the validated PRAMA kernel configuration. Before
D_O distance evaluation, every coordinate at `t` is centered and scaled using only the
previous operator window; the standardized innovation is bounded with `tanh`. Constant
coordinates map to the neutral midpoint. This adapts domain amplitude without modifying
D_O's recurrence, ridge, contraction or hysteresis equations. The adapter is identified as
`financial_gamma_state_adapter_v2_prior_robust_geometry`.

## ODCE v0.1

ODCE is computed causally over trailing windows. It preserves retained friction,
accumulated debt, capacity consumption, excess persistence, adverse trend, structural
recovery, adaptive organization, differentials, differential trends and irreversible
cumulative positive exposure.

Every endogenous cost/benefit channel retains its raw value and receives a bounded
financial magnitude normalized against its own strictly-prior ODCE history. Differentials
use those comparable normalized channels. `functional_gain`, `external_integration`,
`verified_outcome`, and calibrated positive persistence remain `UNAVAILABLE`: no
independent conformant financial source or noise-floor calibration has been supplied.

## K-MEM

The active topology is the reference K1 post-observer channel over D_O transport deficit:

```text
z[t] = exp(-1/32) z[t-1] + (1 - exp(-1/32)) x[t]
```

The structural vector uses `z[t-1]`, not `z[t]`. Both states are retained in the component
artifact so the no-lookahead boundary is auditable.

## Directional calibration

Direction is downstream and cannot alter PRAMA or the structural observers. The offline
builder derives one pooled volatility-normalized barrier and applies it symmetrically to
UP/DOWN, labels only paths strictly after each snapshot, and separates temporal train,
validation and untouched test regions. Normalization is fitted on train; estimator and
publication parameters are selected on validation; test outcomes never tune them.
Dimensions constant on train remain in custody but are excluded from distance.

Runtime requires the profile hash to reproduce, structural vector versions and names to
match, and effective availability masks to match exactly. Reliability uses 95% Wilson lower
bounds, and probability quality must have positive untouched-test Brier skill against the
train+validation climatology. Otherwise it publishes `UNRESOLVED`.

A profile is publication-eligible only when it binds a lowercase SHA-256 of a protocol
frozen before test outcomes were inspected. Profiles built without that custody input are
marked `DEVELOPMENT_AUDIT_CONSUMED`; their diagnostics remain useful, but runtime direction
and horizon are suppressed.

Ergonektim is used here as a product-contract precedent: prefix causality, deterministic
custody, immutable artifacts and fail-closed eligibility. Its electrical observers are not
relabelled as financial D_O/ODCE/K-MEM implementations.
