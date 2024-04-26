//! Tattoy: Eye-candy for your terminal

// =========================================================================
//                  Canonical lints for whole crate
// =========================================================================
// Official docs:
//   https://doc.rust-lang.org/nightly/clippy/lints.html
// Useful app to lookup full details of individual lints:
//   https://rust-lang.github.io/rust-clippy/master/index.html
//
// We set base lints to give the fullest, most pedantic feedback possible.
// Though we prefer that they are just warnings during development so that build-denial
// is only enforced in CI.
//
#![warn(
    // `clippy::all` is already on by default. It implies the following:
    //   clippy::correctness code that is outright wrong or useless
    //   clippy::suspicious code that is most likely wrong or useless
    //   clippy::complexity code that does something simple but in a complex way
    //   clippy::perf code that can be written to run faster
    //   clippy::style code that should be written in a more idiomatic way
    clippy::all,

    // It's always good to write as much documentation as possible
    missing_docs,

    // > clippy::pedantic lints which are rather strict or might have false positives
    clippy::pedantic,

    // > new lints that are still under development
    // (so "nursery" doesn't mean "Rust newbies")
    clippy::nursery,

    // > The clippy::cargo group gives you suggestions on how to improve your Cargo.toml file.
    // > This might be especially interesting if you want to publish your crate and are not sure
    // > if you have all useful information in your Cargo.toml.
    clippy::cargo
)]
// > The clippy::restriction group will restrict you in some way.
// > If you enable a restriction lint for your crate it is recommended to also fix code that
// > this lint triggers on. However, those lints are really strict by design and you might want
// > to #[allow] them in some special cases, with a comment justifying that.
#![allow(clippy::blanket_clippy_restriction_lints)]
#![warn(clippy::restriction)]
//
//
// =========================================================================
//   Individually blanket-allow single lints relevant to this whole crate
// =========================================================================
#![allow(
    // This is idiomatic Rust
    clippy::implicit_return,

    // Multiple dependencies using the same dependency but distinct versions.
    // Are there even projects that don't suffer this?
    clippy::multiple_crate_versions,

    // We're not interested in becoming no-std compatible
    clippy::std_instead_of_alloc,
    clippy::std_instead_of_core,

    clippy::question_mark_used,
    clippy::missing_inline_in_public_items,
    clippy::missing_errors_doc,
    clippy::single_call_fn,
    clippy::absolute_paths,
    clippy::separated_literal_suffix
)]

pub mod loader;
pub mod pty;
pub mod renderer;
pub mod run;
pub mod shadow_tty;
pub mod surface;

/// This is where all the various tattoys are kept
pub mod tattoys {
    pub mod random_walker;
}
