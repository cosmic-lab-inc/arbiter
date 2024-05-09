use sol_chainsaw::ChainsawDeserializer;
use solana_sdk::pubkey::Pubkey;

pub trait DecodeProgramAccount: Sized {
    /// Deserialize a program account into its defined (struct) type using Borsh.
    /// utf8 discriminant is the human-readable discriminant, such as "User", and usually the name
    /// of the struct marked with the #[account] Anchor macro that derives the Discriminator trait.
    fn borsh_decode_account(utf8_discrim: &str, data: &[u8]) -> anyhow::Result<Self>;

    /// Deserialize a program account into a JSON.
    /// utf8 discriminant is the human-readable discriminant, such as "User", and usually the name
    /// of the struct marked with the #[account] Anchor macro that derives the Discriminator trait.
    fn json_decode_account(
        chainsaw: &ChainsawDeserializer<'static>,
        program_id: &Pubkey,
        utf8_discrim: &str,
        data: &mut &[u8],
    ) -> anyhow::Result<serde_json::Value>;
}
