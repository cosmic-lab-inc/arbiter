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
        }
    };
}
