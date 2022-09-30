//! Long Running Services that expose clean APIs and hide implementation details from endpoints
//! If the amount of services grow, add another layer that bundles all services.
mod mail;

pub use mail::ExternalMailRecipient;
pub use mail::MailRecipient;
pub use mail::MailService;
pub use mail::RegisteredMailRecipient;
pub use mail::UnregisteredMailRecipient;
