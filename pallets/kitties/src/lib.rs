#![cfg_attr(not(feature = "std"), no_std)]

use frame_support::{
	pallet_prelude::*,
	traits::{Randomness, Currency, ExistenceRequirement},
	transactional,
};
use frame_system::pallet_prelude::*;
use sp_std::prelude::*;
use sp_io::hashing::blake2_128;
#[cfg(feature = "std")]
use serde::{Deserialize, Serialize};

pub use pallet::*;

#[cfg(test)]
mod tests;
#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;
mod weights;

pub use weights::WeightInfo;

#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
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

#[frame_support::pallet]
pub mod pallet {
	use super::*;

	#[pallet::config]
	pub trait Config: frame_system::Config + orml_nft::Config<TokenData = Kitty, ClassData = ()> {
		type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;
		type Randomness: Randomness<Self::Hash, Self::BlockNumber>;
		type Currency: Currency<Self::AccountId>;
		type WeightInfo: WeightInfo;
	}

	pub type KittyIndexOf<T> = <T as orml_nft::Config>::TokenId;
	pub type BalanceOf<T> = <<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;

	/// Get kitty price. None means not for sale.
	#[pallet::storage]
	#[pallet::getter(fn kitty_prices)]
	pub type KittyPrices<T: Config> = StorageMap<
		_,
		Blake2_128Concat, KittyIndexOf<T>,
		BalanceOf<T>, OptionQuery
	>;

	/// The class id for orml_nft
	#[pallet::storage]
	#[pallet::getter(fn class_id)]
	pub type ClassId<T: Config> = StorageValue<_, T::ClassId, ValueQuery>;

	#[pallet::genesis_config]
	#[derive(Default)]
	pub struct GenesisConfig;

	#[pallet::genesis_build]
	impl<T: Config> GenesisBuild<T> for GenesisConfig {
		fn build(&self) {
			// create a NTF class
			let class_id = orml_nft::Pallet::<T>::create_class(&Default::default(), Vec::new(), ())
				.expect("Cannot fail or invalid chain spec");
			ClassId::<T>::put(class_id);
		}
	}

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	#[pallet::metadata(
		T::AccountId = "AccountId", KittyIndexOf<T> = "KittyIndex", Option<BalanceOf<T>> = "Option<Balance>", BalanceOf<T> = "Balance",
	)]
	pub enum Event<T: Config> {
		/// A kitty is created. \[owner, kitty_id, kitty\]
		KittyCreated(T::AccountId, KittyIndexOf<T>, Kitty),
		/// A new kitten is bred. \[owner, kitty_id, kitty\]
		KittyBred(T::AccountId, KittyIndexOf<T>, Kitty),
		/// A kitty is transferred. \[from, to, kitty_id\]
		KittyTransferred(T::AccountId, T::AccountId, KittyIndexOf<T>),
		/// The price for a kitty is updated. \[owner, kitty_id, price\]
		KittyPriceUpdated(T::AccountId, KittyIndexOf<T>, Option<BalanceOf<T>>),
		/// A kitty is sold. \[old_owner, new_owner, kitty_id, price\]
		KittySold(T::AccountId, T::AccountId, KittyIndexOf<T>, BalanceOf<T>),
	}

	#[pallet::error]
	pub enum Error<T> {
		InvalidKittyId,
		SameGender,
		NotOwner,
		NotForSale,
		PriceTooLow,
		BuyFromSelf,
	}

	#[pallet::pallet]
	#[pallet::generate_store(pub(super) trait Store)]
	pub struct Pallet<T>(_);

	#[pallet::call]
	impl<T:Config> Pallet<T> {

		/// Create a new kitty
		#[pallet::weight(T::WeightInfo::create())]
		pub fn create(origin: OriginFor<T>) -> DispatchResult {
			let sender = ensure_signed(origin)?;

			let dna = Self::random_value(&sender);

			// Create and store kitty
			let kitty = Kitty(dna);
			let kitty_id = orml_nft::Pallet::<T>::mint(&sender, Self::class_id(), Vec::new(), kitty.clone())?;

			// Emit event
			Self::deposit_event(Event::KittyCreated(sender, kitty_id, kitty));

			Ok(())
		}

		/// Breed kitties
		#[pallet::weight(T::WeightInfo::breed())]
		pub fn breed(origin: OriginFor<T>, kitty_id_1: KittyIndexOf<T>, kitty_id_2: KittyIndexOf<T>) -> DispatchResult {
			let sender = ensure_signed(origin)?;
			let kitty1 = Self::kitties(&sender, kitty_id_1).ok_or(Error::<T>::InvalidKittyId)?;
			let kitty2 = Self::kitties(&sender, kitty_id_2).ok_or(Error::<T>::InvalidKittyId)?;

			ensure!(kitty1.gender() != kitty2.gender(), Error::<T>::SameGender);

			let kitty1_dna = kitty1.0;
			let kitty2_dna = kitty2.0;

			let selector = Self::random_value(&sender);
			let mut new_dna = [0u8; 16];

			// Combine parents and selector to create new kitty
			for i in 0..kitty1_dna.len() {
				new_dna[i] = combine_dna(kitty1_dna[i], kitty2_dna[i], selector[i]);
			}

			let new_kitty = Kitty(new_dna);

			let kitty_id = orml_nft::Pallet::<T>::mint(&sender, Self::class_id(), Vec::new(), new_kitty.clone())?;

			Self::deposit_event(Event::KittyBred(sender, kitty_id, new_kitty));

			Ok(())
		}

		/// Transfer a kitty to new owner
		#[pallet::weight(T::WeightInfo::transfer())]
		pub fn transfer(origin: OriginFor<T>, to: T::AccountId, kitty_id: KittyIndexOf<T>) -> DispatchResult {
			let sender = ensure_signed(origin)?;

			orml_nft::Pallet::<T>::transfer(&sender, &to, (Self::class_id(), kitty_id))?;

			if sender != to {
				KittyPrices::<T>::remove(kitty_id);

				Self::deposit_event(Event::KittyTransferred(sender, to, kitty_id));
			}

			Ok(())
		}

		/// Set a price for a kitty for sale
 		/// None to delist the kitty
		#[pallet::weight(T::WeightInfo::set_price())]
		pub fn set_price(origin: OriginFor<T>, kitty_id: KittyIndexOf<T>, new_price: Option<BalanceOf<T>>) -> DispatchResult {
			let sender = ensure_signed(origin)?;

			ensure!(orml_nft::TokensByOwner::<T>::contains_key(&sender, (Self::class_id(), kitty_id)), Error::<T>::NotOwner);

			KittyPrices::<T>::mutate_exists(kitty_id, |price| *price = new_price);

			Self::deposit_event(Event::KittyPriceUpdated(sender, kitty_id, new_price));

			Ok(())
		}

		/// Buy a kitty
		#[pallet::weight(T::WeightInfo::buy())]
		#[transactional]
		pub fn buy(origin: OriginFor<T>, owner: T::AccountId, kitty_id: KittyIndexOf<T>, max_price: BalanceOf<T>) -> DispatchResult {
			let sender = ensure_signed(origin)?;

			ensure!(sender != owner, Error::<T>::BuyFromSelf);

			KittyPrices::<T>::try_mutate_exists(kitty_id, |price| -> DispatchResult {
				let price = price.take().ok_or(Error::<T>::NotForSale)?;

				ensure!(max_price >= price, Error::<T>::PriceTooLow);

				orml_nft::Pallet::<T>::transfer(&owner, &sender, (Self::class_id(), kitty_id))?;
				T::Currency::transfer(&sender, &owner, price, ExistenceRequirement::KeepAlive)?;

				Self::deposit_event(Event::KittySold(owner, sender, kitty_id, price));

				Ok(())
			})
		}
	}
}

fn combine_dna(dna1: u8, dna2: u8, selector: u8) -> u8 {
	(!selector & dna1) | (selector & dna2)
}

impl<T: Config> Pallet<T> {
	fn kitties(owner: &T::AccountId, kitty_id: KittyIndexOf<T>) -> Option<Kitty> {
		orml_nft::Pallet::<T>::tokens(Self::class_id(), kitty_id).and_then(|x| {
			if x.owner == *owner {
				Some(x.data)
			} else {
				None
			}
		})
	}

	fn random_value(sender: &T::AccountId) -> [u8; 16] {
		let payload = (
			T::Randomness::random_seed().0,
			&sender,
			<frame_system::Pallet<T>>::extrinsic_index(),
		);
		payload.using_encoded(blake2_128)
	}
}
