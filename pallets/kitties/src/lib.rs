#![cfg_attr(not(feature = "std"), no_std)]

use codec::{Encode, Decode};
use frame_support::{
	decl_module, decl_storage, decl_event, decl_error, ensure, StorageValue,
	traits::{Randomness, Currency, ExistenceRequirement, Get}, RuntimeDebug, dispatch::DispatchResult,
};
use sp_io::hashing::blake2_128;
use frame_system::{ensure_signed, ensure_none};
use sp_std::vec::Vec;
use sp_runtime::{
	transaction_validity::{
		InvalidTransaction, TransactionSource, TransactionValidity, ValidTransaction,
	},
};
use orml_utilities::with_transaction_result;
use orml_nft::Module as NftModule;
#[cfg(feature = "std")]
use serde::{Deserialize, Serialize};

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

pub trait Config: orml_nft::Config<TokenData = Kitty, ClassData = ()> {
	type Event: From<Event<Self>> + Into<<Self as frame_system::Config>::Event>;
	type Randomness: Randomness<Self::Hash>;
	type Currency: Currency<Self::AccountId>;
	type WeightInfo: WeightInfo;
	type DefaultDifficulty: Get<u32>;
}

type KittyIndexOf<T> = <T as orml_nft::Config>::TokenId;
type BalanceOf<T> = <<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;

decl_storage! {
	trait Store for Module<T: Config> as Kitties {
		/// Get kitty price. None means not for sale.
		pub KittyPrices get(fn kitty_prices): map hasher(blake2_128_concat) KittyIndexOf<T> => Option<BalanceOf<T>>;
		/// The class id for orml_nft
		pub ClassId get(fn class_id): T::ClassId;
		/// Nonce for auto breed to prevent replay attack
		pub AutoBreedNonce get(fn auto_breed_nonce): u32;
	}
	add_extra_genesis {
		build(|_config| {
			// create a NTF class
			let class_id = NftModule::<T>::create_class(&Default::default(), Vec::new(), ()).expect("Cannot fail or invalid chain spec");
			ClassId::<T>::put(class_id);
		})
	}
}

decl_event! {
	pub enum Event<T> where
		<T as frame_system::Config>::AccountId,
		KittyIndex = KittyIndexOf<T>,
		Balance = BalanceOf<T>,
	{
		/// A kitty is created. \[owner, kitty_id, kitty\]
		KittyCreated(AccountId, KittyIndex, Kitty),
		/// A new kitten is bred. \[owner, kitty_id, kitty\]
		KittyBred(AccountId, KittyIndex, Kitty),
		/// A kitty is transferred. \[from, to, kitty_id\]
		KittyTransferred(AccountId, AccountId, KittyIndex),
		/// The price for a kitty is updated. \[owner, kitty_id, price\]
		KittyPriceUpdated(AccountId, KittyIndex, Option<Balance>),
		/// A kitty is sold. \[old_owner, new_owner, kitty_id, price\]
		KittySold(AccountId, AccountId, KittyIndex, Balance),
	}
}

decl_error! {
	pub enum Error for Module<T: Config> {
		InvalidKittyId,
		SameGender,
		NotOwner,
		NotForSale,
		PriceTooLow,
		BuyFromSelf,
	}
}

decl_module! {
	pub struct Module<T: Config> for enum Call where origin: T::Origin {
		type Error = Error<T>;

		/// Default difficulty for auto breed
		const DefaultDifficulty: u32 = T::DefaultDifficulty::get();

		fn deposit_event() = default;

		/// Create a new kitty
		#[weight = T::WeightInfo::create()]
		pub fn create(origin) {
			let sender = ensure_signed(origin)?;
			let dna = Self::random_value(&sender);

			// Create and store kitty
			let kitty = Kitty(dna);
			let kitty_id = NftModule::<T>::mint(&sender, Self::class_id(), Vec::new(), kitty.clone())?;

			// Emit event
			Self::deposit_event(RawEvent::KittyCreated(sender, kitty_id, kitty));
		}

		/// Breed kitties
		#[weight = T::WeightInfo::breed()]
		pub fn breed(origin, kitty_id_1: KittyIndexOf<T>, kitty_id_2: KittyIndexOf<T>) {
			let sender = ensure_signed(origin)?;

			let kitty1 = Self::kitties(&sender, kitty_id_1).ok_or(Error::<T>::InvalidKittyId)?;
			let kitty2 = Self::kitties(&sender, kitty_id_2).ok_or(Error::<T>::InvalidKittyId)?;

			Self::do_breed(sender, kitty1, kitty2)?;
		}

		/// Transfer a kitty to new owner
		#[weight = T::WeightInfo::transfer()]
		pub fn transfer(origin, to: T::AccountId, kitty_id: KittyIndexOf<T>) {
			let sender = ensure_signed(origin)?;

			NftModule::<T>::transfer(&sender, &to, (Self::class_id(), kitty_id))?;

			if sender != to {
				KittyPrices::<T>::remove(kitty_id);

				Self::deposit_event(RawEvent::KittyTransferred(sender, to, kitty_id));
			}
		}

		/// Set a price for a kitty for sale
		/// None to delist the kitty
		#[weight = T::WeightInfo::set_price()]
		pub fn set_price(origin, kitty_id: KittyIndexOf<T>, new_price: Option<BalanceOf<T>>) {
			let sender = ensure_signed(origin)?;

			ensure!(orml_nft::TokensByOwner::<T>::contains_key(&sender, (Self::class_id(), kitty_id)), Error::<T>::NotOwner);

			KittyPrices::<T>::mutate_exists(kitty_id, |price| *price = new_price);

			Self::deposit_event(RawEvent::KittyPriceUpdated(sender, kitty_id, new_price));
		}

		/// Buy a kitty
		#[weight = T::WeightInfo::buy()]
		pub fn buy(origin, owner: T::AccountId, kitty_id: KittyIndexOf<T>, max_price: BalanceOf<T>) {
			let sender = ensure_signed(origin)?;

			ensure!(sender != owner, Error::<T>::BuyFromSelf);

			KittyPrices::<T>::try_mutate_exists(kitty_id, |price| -> DispatchResult {
				let price = price.take().ok_or(Error::<T>::NotForSale)?;

				ensure!(max_price >= price, Error::<T>::PriceTooLow);

				with_transaction_result(|| {
					NftModule::<T>::transfer(&owner, &sender, (Self::class_id(), kitty_id))?;
					T::Currency::transfer(&sender, &owner, price, ExistenceRequirement::KeepAlive)?;

					Self::deposit_event(RawEvent::KittySold(owner, sender, kitty_id, price));

					Ok(())
				})
			})?;
		}

		#[weight = 1000]
		pub fn auto_breed(origin, kitty_id_1: KittyIndexOf<T>, kitty_id_2: KittyIndexOf<T>, _nonce: u32, _solution: u128) {
			ensure_none(origin)?;

			let kitty1 = NftModule::<T>::tokens(Self::class_id(), kitty_id_1).ok_or(Error::<T>::InvalidKittyId)?;
			let kitty2 = NftModule::<T>::tokens(Self::class_id(), kitty_id_2).ok_or(Error::<T>::InvalidKittyId)?;

			Self::do_breed(kitty1.owner, kitty1.data, kitty2.data)?;
		}
	}
}

fn combine_dna(dna1: u8, dna2: u8, selector: u8) -> u8 {
	(!selector & dna1) | (selector & dna2)
}

impl<T: Config> Module<T> {
	fn kitties(owner: &T::AccountId, kitty_id: KittyIndexOf<T>) -> Option<Kitty> {
		NftModule::<T>::tokens(Self::class_id(), kitty_id).and_then(|x| {
			if x.owner == *owner {
				Some(x.data)
			} else {
				None
			}
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
		let kitty_id = NftModule::<T>::mint(&owner, Self::class_id(), Vec::new(), new_kitty.clone())?;

		Self::deposit_event(RawEvent::KittyBred(owner, kitty_id, new_kitty));

		Ok(())
	}

	fn validate_solution(kitty_id_1: KittyIndexOf<T>, kitty_id_2: KittyIndexOf<T>, nonce: u32, solution: u128) -> bool {
		let payload = (kitty_id_1, kitty_id_2, nonce, solution);
		let hash = payload.using_encoded(blake2_128);
		let hash_value = u128::from_le_bytes(hash);
		let difficulty = T::DefaultDifficulty::get();

		hash_value < (u128::max_value() / difficulty as u128)
	}
}

impl<T: Config> frame_support::unsigned::ValidateUnsigned for Module<T> {
	type Call = Call<T>;

	fn validate_unsigned(_source: TransactionSource, call: &Self::Call) -> TransactionValidity {
		match *call {
			Call::auto_breed(kitty_id_1, kitty_id_2, nonce, solution) => {
				if Self::validate_solution(kitty_id_1, kitty_id_2, nonce, solution) {
					if nonce != Self::auto_breed_nonce() {
						return InvalidTransaction::BadProof.into();
					}

					AutoBreedNonce::mutate(|nonce| *nonce = nonce.saturating_add(1));

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
