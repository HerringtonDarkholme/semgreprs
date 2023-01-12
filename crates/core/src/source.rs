use std::ops::Deref;

pub trait Content: ToString + Deref<Target = str> {
  fn as_mut_vec(&mut self) -> &mut Vec<u8>;
}

pub enum Source {
  Plain(String),
  Customized(Box<dyn Content + Sync + Send>),
}

use Source::*;

impl From<&str> for Source {
  fn from(s: &str) -> Self {
    Plain(s.into())
  }
}

impl Clone for Source {
  fn clone(&self) -> Self {
    todo!()
  }
}

impl Deref for Source {
  type Target = str;
  fn deref(&self) -> &Self::Target {
    match self {
      Plain(s) => s.deref(),
      Customized(c) => c.deref(),
    }
  }
}

impl ToString for Source {
  fn to_string(&self) -> String {
    match self {
      Self::Plain(s) => s.to_owned(),
      Self::Customized(c) => c.to_string(),
    }
  }
}

impl Content for Source {
  fn as_mut_vec(&mut self) -> &mut Vec<u8> {
    match self {
      Plain(s) => unsafe { s.as_mut_vec() },
      Customized(c) => c.as_mut_vec(),
    }
  }
}
