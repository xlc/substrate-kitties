#![cfg_attr(not(feature = "std"), no_std)]

use frame_support::decl_module;

pub trait Config: frame_system::Config {
}

decl_module! {
	pub struct Module<T: Config> for enum Call where origin: T::Origin {
	}
}
