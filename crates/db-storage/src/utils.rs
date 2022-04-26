use crate::users::UserId;

/// Trait for models that have user-ids attached to them like created_by/updated_by fields
///
/// Used to make batch requests of users after fetching some resources
///
/// Should only be implemented on references of the actual models
pub trait HasUsers {
    fn populate(self, dst: &mut Vec<UserId>);
}

impl<T, I> HasUsers for I
where
    T: HasUsers,
    I: IntoIterator<Item = T>,
{
    fn populate(self, dst: &mut Vec<UserId>) {
        for t in self {
            t.populate(dst);
        }
    }
}
