#[macro_export]
macro_rules! trunc {
    ($num:expr, $decimals:expr) => {{
        let factor = 10.0_f64.powi($decimals);
        ($num * factor).round() / factor
    }};
}

#[macro_export]
macro_rules! decode_account {
    ($vis:vis enum $ident:ident {
        $($variant:ident ($account_type:ty)),*$(,)?
    }) => {
        #[repr(C)]
        #[derive(anchor_lang::prelude::AnchorDeserialize, anchor_lang::prelude::AnchorSerialize)]
        #[derive(Copy, Clone)]
        $vis enum $ident {
            $($variant($account_type),)*
        }

        impl $crate::DecodeProgramAccount for $ident {
            fn borsh_decode_account(utf8_discrim: &str, data: &[u8]) -> anyhow::Result<Self> {
                match utf8_discrim {
                    $(
                      $variant if utf8_discrim == $crate::get_type_name::<$account_type>() => {
                          let name = $crate::get_type_name::<$account_type>();
                          let acct = <$account_type>::try_from_slice(&data[8..])?;
                          Ok(Self::$variant(acct.clone()))
                      },
                    )*
                    _ => Err(anyhow::anyhow!("Invalid account discriminant")),
                }
            }

            fn json_decode_account(
                chainsaw: &sol_chainsaw::ChainsawDeserializer<'static>,
                program_id: &solana_sdk::pubkey::Pubkey,
                utf8_discrim: &str,
                data: &mut &[u8]
            ) -> anyhow::Result<serde_json::Value> {
                match utf8_discrim {
                    $(
                      $variant if utf8_discrim == $crate::get_type_name::<$account_type>() => {
                          let name = $crate::get_type_name::<$account_type>();
                          // TODO: this errors on packed structs because it desers with Borsh first...
                          let str = chainsaw
                              .deserialize_account_to_json_string(&program_id.to_string(), data)?;
                          let acct = serde_json::from_str::<serde_json::Value>(&str)?;
                          Ok(acct)
                      },
                    )*
                    _ => Err(anyhow::anyhow!("Invalid account discriminant")),
                }
            }
        }
    };
}
