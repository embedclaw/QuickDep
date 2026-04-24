//! Model definitions

/// User model
pub struct User {
    pub name: String,
    pub age: Option<u32>,
}

impl User {
    /// Create a new user
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            age: None,
        }
    }
    
    /// Set age
    pub fn with_age(mut self, age: u32) -> Self {
        self.age = Some(age);
        self
    }
}

/// Status enum
pub enum Status {
    Active,
    Inactive,
}
