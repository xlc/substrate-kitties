#![cfg_attr(not(feature = "std"), no_std)]

use codec::{Encode, Decode};
use frame_support::{
	decl_module, decl_storage, decl_event, decl_error, ensure, StorageValue, StorageDoubleMap, Parameter,
	traits::Randomness, RuntimeDebug, dispatch::{DispatchError, DispatchResult},
};
use sp_io::hashing::blake2_128;
use frame_system::ensure_signed;
use sp_runtime::traits::{AtLeast32BitUnsigned, Bounded, One, CheckedAdd};

#[cfg(test)]
mod tests;

#[derive(Encode, Decode, Clone, RuntimeDebug, PartialEq, Eq)]
pub struct Kitty(pub [u8; 16]);

#[derive(Encode, Decode, Clone, Copy, RuntimeDebug, PartialEq, Eq)]
pub enum KittyGender {
	Male,
	Female,
}

impl Kitty {
	pub fn gender(&self) -> KittyGender {
		if self.0[0] % 2 == 0 {
			KittyGender::Male
		} else {
			KittyGender::Female
		}
	}
}

pub trait Config: frame_system::Config {
	type Event: From<Event<Self>> + Into<<Self as frame_system::Config>::Event>;
	type Randomness: Randomness<Self::Hash>;
	type KittyIndex: Parameter + AtLeast32BitUnsigned + Bounded + Default + Copy;
}

decl_storage! {
	trait Store for Module<T: Config> as Kitties {
		/// Stores all the kitties, key is the kitty id
		pub Kitties get(fn kitties): double_map hasher(blake2_128_concat) T::AccountId, hasher(blake2_128_concat) T::KittyIndex => Option<Kitty>;
		/// Stores the next kitty ID
		pub NextKittyId get(fn next_kitty_id): T::KittyIndex;
	}
}

decl_event! {
	pub enum Event<T> where
		<T as frame_system::Config>::AccountId,
		<T as Config>::KittyIndex,
	{
		/// A kitty is created. \[owner, kitty_id, kitty\]
		KittyCreated(AccountId, KittyIndex, Kitty),
		/// A new kitten is bred. \[owner, kitty_id, kitty\]
		KittyBred(AccountId, KittyIndex, Kitty),
		/// A kitty is transferred. \[from, to, kitty_id\]
		KittyTransferred(AccountId, AccountId, KittyIndex),
	}
}

decl_error! {
	pub enum Error for Module<T: Config> {
		KittiesIdOverflow,
		InvalidKittyId,
		SameGender,
	}
}

decl_module! {
	pub struct Module<T: Config> for enum Call where origin: T::Origin {
		type Error = Error<T>;

		fn deposit_event() = default;

		/// Create a new kitty
		#[weight = 1000]
		pub fn create(origin) {
			let sender = ensure_signed(origin)?;

			let kitty_id = Self::get_next_kitty_id()?;

			let dna = Self::random_value(&sender);

			// Create and store kitty
			let kitty = Kitty(dna);
			Kitties::<T>::insert(&sender, kitty_id, &kitty);

			// Emit event
			Self::deposit_event(RawEvent::KittyCreated(sender, kitty_id, kitty));
		}

		/// Breed kitties
		#[weight = 1000]
		pub fn breed(origin, kitty_id_1: T::KittyIndex, kitty_id_2: T::KittyIndex) {
			let sender = ensure_signed(origin)?;
			let kitty1 = Self::kitties(&sender, kitty_id_1).ok_or(Error::<T>::InvalidKittyId)?;
			let kitty2 = Self::kitties(&sender, kitty_id_2).ok_or(Error::<T>::InvalidKittyId)?;

			ensure!(kitty1.gender() != kitty2.gender(), Error::<T>::SameGender);

			let kitty_id = Self::get_next_kitty_id()?;

			let kitty1_dna = kitty1.0;
			let kitty2_dna = kitty2.0;

			let selector = Self::random_value(&sender);
			let mut new_dna = [0u8; 16];

			// Combine parents and selector to create new kitty
			for i in 0..kitty1_dna.len() {
				new_dna[i] = combine_dna(kitty1_dna[i], kitty2_dna[i], selector[i]);
			}

			let new_kitty = Kitty(new_dna);

			Kitties::<T>::insert(&sender, kitty_id, &new_kitty);

			Self::deposit_event(RawEvent::KittyBred(sender, kitty_id, new_kitty));
		}

		/// Transfer a kitty to new owner
		#[weight = 1000]
		pub fn transfer(origin, to: T::AccountId, kitty_id: T::KittyIndex) {
			let sender = ensure_signed(origin)?;

			Kitties::<T>::try_mutate_exists(sender.clone(), kitty_id, |kitty| -> DispatchResult {
				if sender == to {
					ensure!(kitty.is_some(), Error::<T>::InvalidKittyId);
					return Ok(());
				}

				let kitty = kitty.take().ok_or(Error::<T>::InvalidKittyId)?;

				Kitties::<T>::insert(&to, kitty_id, kitty);

				Self::deposit_event(RawEvent::KittyTransferred(sender, to, kitty_id));

				Ok(())
			})?;
		}
	}
}

fn combine_dna(dna1: u8, dna2: u8, selector: u8) -> u8 {
	(!selector & dna1) | (selector & dna2)
}

impl<T: Config> Module<T> {
	fn get_next_kitty_id() -> sp_std::result::Result<T::KittyIndex, DispatchError> {
		NextKittyId::<T>::try_mutate(|next_id| -> sp_std::result::Result<T::KittyIndex, DispatchError> {
			let current_id = *next_id;
			*next_id = next_id.checked_add(&One::one()).ok_or(Error::<T>::KittiesIdOverflow)?;
			Ok(current_id)
		})
	}

	fn random_value(sender: &T::AccountId) -> [u8; 16] {
		let payload = (
			T::Randomness::random_seed(),
			&sender,
			<frame_system::Module<T>>::extrinsic_index(),
		);
		payload.using_encoded(blake2_128)
	}
}
