#![cfg_attr(not(feature = "std"), no_std)]

use frame_support::{
	pallet_prelude::*,
	traits::{Randomness, Currency, ExistenceRequirement},
	transactional,
};
use frame_system::{
	pallet_prelude::*,
	offchain::{SendTransactionTypes, SubmitTransaction},
};
use sp_std::{
	prelude::*,
	convert::TryInto
};
use sp_io::hashing::blake2_128;
use sp_runtime::offchain::storage_lock::{StorageLock, BlockAndTime};
use rand_chacha::{
	rand_core::{RngCore, SeedableRng},
	ChaChaRng,
};
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
	pub trait Config: frame_system::Config + orml_nft::Config<TokenData = Kitty, ClassData = ()> + SendTransactionTypes<Call<Self>> {
		type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;
		type Randomness: Randomness<Self::Hash, Self::BlockNumber>;
		type Currency: Currency<Self::AccountId>;
		type WeightInfo: WeightInfo;
		#[pallet::constant]
		type DefaultDifficulty: Get<u32>;
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

	/// Nonce for auto breed to prevent replay attack
	#[pallet::storage]
	#[pallet::getter(fn auto_breed_nonce)]
	pub type AutoBreedNonce<T: Config> = StorageValue<_, u32, ValueQuery>;

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

	#[pallet::hooks]
	impl<T: Config> Hooks<T::BlockNumber> for Pallet<T> {
		fn offchain_worker(_now: T::BlockNumber) {
			let _ = Self::run_offchain_worker();
		}
	}

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

			Self::do_breed(sender, kitty1, kitty2)
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

		#[pallet::weight(1000)]
		pub fn auto_breed(origin: OriginFor<T>, kitty_id_1: KittyIndexOf<T>, kitty_id_2: KittyIndexOf<T>, _nonce: u32, _solution: u128) -> DispatchResult {
			ensure_none(origin)?;

			let kitty1 = orml_nft::Pallet::<T>::tokens(Self::class_id(), kitty_id_1).ok_or(Error::<T>::InvalidKittyId)?;
			let kitty2 = orml_nft::Pallet::<T>::tokens(Self::class_id(), kitty_id_2).ok_or(Error::<T>::InvalidKittyId)?;

			Self::do_breed(kitty1.owner, kitty1.data, kitty2.data)
		}
	}

	#[pallet::validate_unsigned]
	impl<T: Config> frame_support::unsigned::ValidateUnsigned for Pallet<T> {
		type Call = Call<T>;

		fn validate_unsigned(_source: TransactionSource, call: &Self::Call) -> TransactionValidity {
			match *call {
				Call::auto_breed(kitty_id_1, kitty_id_2, nonce, solution) => {
					if Self::validate_solution(kitty_id_1, kitty_id_2, nonce, solution) {
						if nonce != Self::auto_breed_nonce() {
							return InvalidTransaction::BadProof.into();
						}

						AutoBreedNonce::<T>::mutate(|nonce| *nonce = nonce.saturating_add(1));

						ValidTransaction::with_tag_prefix("kitties")
							.longevity(64_u64)
							.propagate(true)
							.build()
					} else {
						InvalidTransaction::BadProof.into()
					}
				},
				_ => InvalidTransaction::Call.into(),
			}
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

	fn do_breed(
		owner: T::AccountId,
		kitty1: Kitty,
		kitty2: Kitty,
	) -> DispatchResult {
		ensure!(kitty1.gender() != kitty2.gender(), Error::<T>::SameGender);

		let kitty1_dna = kitty1.0;
		let kitty2_dna = kitty2.0;

		let selector = Self::random_value(&owner);
		let mut new_dna = [0u8; 16];

		// Combine parents and selector to create new kitty
		for i in 0..kitty1_dna.len() {
			new_dna[i] = combine_dna(kitty1_dna[i], kitty2_dna[i], selector[i]);
		}

		let new_kitty = Kitty(new_dna);
		let kitty_id = orml_nft::Pallet::<T>::mint(&owner, Self::class_id(), Vec::new(), new_kitty.clone())?;

		Self::deposit_event(Event::KittyBred(owner, kitty_id, new_kitty));

		Ok(())
	}

	fn validate_solution(kitty_id_1: KittyIndexOf<T>, kitty_id_2: KittyIndexOf<T>, nonce: u32, solution: u128) -> bool {
		let payload = (kitty_id_1, kitty_id_2, nonce, solution);
		let hash = payload.using_encoded(blake2_128);
		let hash_value = u128::from_le_bytes(hash);
		let difficulty = T::DefaultDifficulty::get();

		hash_value < (u128::max_value() / difficulty as u128)
	}

	fn run_offchain_worker() -> Result<(), ()> {
		let mut lock = StorageLock::<'_, BlockAndTime<frame_system::Pallet<T>>>::with_block_deadline(&b"kitties/lock"[..], 1);
		let _guard = lock.try_lock().map_err(|_| ())?;

		let random_seed = sp_io::offchain::random_seed();
		let mut rng = ChaChaRng::from_seed(random_seed);

		// this only support if kitty_count <= u32::max_value()
		let kitty_count = TryInto::<u32>::try_into(orml_nft::Pallet::<T>::next_token_id(Self::class_id())).map_err(|_| ())?;

		if kitty_count == 0 {
			return Ok(());
		}

		const MAX_ITERATIONS: u128 = 500;

		let nonce = Self::auto_breed_nonce();

		let mut remaining_iterations = MAX_ITERATIONS;

		let (kitty_1, kitty_2) = loop {
			let kitty_id_1: KittyIndexOf<T> = (rng.next_u32() % kitty_count).into();
			let kitty_id_2: KittyIndexOf<T> = (rng.next_u32() % kitty_count).into();

			let kitty_1 = orml_nft::Pallet::<T>::tokens(Self::class_id(), kitty_id_1).ok_or(())?;
			let kitty_2 = orml_nft::Pallet::<T>::tokens(Self::class_id(), kitty_id_2).ok_or(())?;

			if kitty_1.data.gender() != kitty_2.data.gender() {
				break (kitty_id_1, kitty_id_2);
			}

			remaining_iterations -= 1;

			if remaining_iterations == 0 {
				return Err(());
			}
		};

		let solution_prefix = rng.next_u32() as u128;

		for i in 0 .. remaining_iterations {
			let solution = (solution_prefix << 32) + i;
			if Self::validate_solution(kitty_1, kitty_2, nonce, solution) {
				let _ = SubmitTransaction::<T, Call<T>>::submit_unsigned_transaction(Call::<T>::auto_breed(kitty_1, kitty_2, nonce, solution).into());
				break;
			}
		}

		Ok(())
	}
}
