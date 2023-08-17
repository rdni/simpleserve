//! Contains all errors used in the crate

use std::{error::Error, fmt::Display};

/// An error that occurs when a `Option` is unwrapped
/// 
/// # Examples
/// ```
/// use sserve::errors::OptionUnwrapError;
/// 
/// fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let option: Option<u32> = Some(54);
///     let unwrapped = match option {
///         Some(_) => {return Ok(());},
///         None => {return Err(Box::new(OptionUnwrapError {}));}
///     };
///     Ok(())
/// }
#[derive(Debug)]
pub struct OptionUnwrapError;

impl Display for OptionUnwrapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "`Option::unwrap()` failed")
    }
}
impl Error for OptionUnwrapError {}

