mod proto;
use std::{path::Path, io, string::FromUtf8Error};

use heck::{ToPascalCase, ToSnakeCase, ToShoutySnakeCase};
use proc_macro2::{TokenStream, Ident, Span};
pub use proto::*;
use quote::quote;

pub type Result<T> = core::result::Result<T, Error>;
#[derive(Debug)]
pub enum Error {
    Toml(toml::de::Error),
    Io(io::Error),
    Utf8(FromUtf8Error)
}
impl From<toml::de::Error> for Error {
    fn from(error: toml::de::Error) -> Self {
        Self::Toml(error)
    }
}
impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
impl From<FromUtf8Error> for Error {
    fn from(error: FromUtf8Error) -> Self {
        Self::Utf8(error)
    }
}

pub fn protocol<P: AsRef<Path>>(path: P) -> Result<TokenStream> {
    let protocol = proto::Protocol::load(path)?;
    let header = format!("# {}", protocol.name);
    let summary = protocol.summary.map(|summary| quote! {#![doc = #summary]});
    let description = protocol.description.map(|description| quote! {#![doc = #description]});
    let copyright = protocol.copyright.map(|copyright| quote! {
        #![doc = "## Copyright"]
        #![doc = #copyright]
    });

    let interfaces = protocol.interfaces.into_iter().map(|i| interface(i));

    Ok(quote!{
        #![doc = #header]
        #summary
        #![doc = ""]
        #description
        #copyright
        
        #(#interfaces)*
    })
}

pub fn interface(interface: Interface) -> TokenStream {
    let trait_ident = Ident::new_raw(&interface.name.to_pascal_case(), Span::call_site());
    let mod_ident = Ident::new_raw(&interface.name.to_snake_case(), Span::call_site());
    let name = &interface.name;
    let version = interface.version;
    let version_doc = format!("`Version {}`", interface.version);
    let summary = interface.summary.as_ref().map(|summary| quote!{#[doc = #summary]});
    let description = interface.description.as_ref().map(|description| quote! {#[doc = #description]});

    let enums = interface.enums.iter().map(|e| enumeration(e));
    let requests = interface.requests.iter().map(|r| request(r));
    let events = interface.events.iter().enumerate().map(|(opcode, e)| event(&interface, e, opcode.try_into().unwrap()));

    let dispatch_requests = interface.requests.iter().enumerate().map(|(opcode, r)| {
        let opcode: u16 = opcode.try_into().unwrap();
        let request_name = &r.name.to_snake_case();
        let ident = Ident::new_raw(request_name, Span::call_site());
        let stream = Ident::new("_stream", Span::call_site());

        let define_args = r.args.iter().map(|a| {
            let ident = Ident::new_raw(&a.name.to_snake_case(), Span::call_site());
            let getter = a.getter(&stream);
            quote!{let #ident = #getter;}
        });
        let args = r.args.iter().map(|a| {
            let ident = Ident::new_raw(&a.name.to_snake_case(), Span::call_site());
            quote!{#ident}
        });
        let args_debug_idents = r.args.iter().map(|a| {
            let ident = Ident::new_raw(&a.name.to_snake_case(), Span::call_site());
            quote!{#ident}
        });
        let args_debug_templates = r.args.iter().enumerate().map(|(i, _)| {
            if i == 0 {
                quote!{"{:?}"}
            } else {
                quote!{", {:?}"}
            }
        });
        quote!{
            #opcode => {
                let #stream = _client.stream();
                #(#define_args)*
                #[cfg(debug_assertions)]
                {
                    ::std::println!(::std::concat!(#name, "@{}.", #request_name, "(", #(#args_debug_templates,)* ")"), _this.id(), #(#args_debug_idents,)*);
                }
                Self::#ident(_this, _event_loop, _client #(, #args)*)
            }
        }
    });

    quote!{
        #summary
        #[doc = ""]
        #[doc = #version_doc]
        #[doc = ""]
        #description
        pub trait #trait_ident<T>: 'static + ::core::marker::Sized {
            const INTERFACE: &'static ::core::primitive::str = #name;
            const VERSION: ::core::primitive::u32 = #version;
            #[doc(hidden)]
            fn dispatch(_this: ::yutani::lease::Lease<dyn ::core::any::Any>, _event_loop: &mut ::yutani::wire::EventLoop<T>, _client: &mut ::yutani::server::Client<T>, _message: ::yutani::wire::Message) -> ::core::result::Result<(), ::yutani::wire::WlError<'static>> {
                let _this: ::yutani::lease::Lease<Self> = _this.downcast().ok_or(::yutani::wire::WlError::INTERNAL)?;
                match _message.opcode {
                    #(#dispatch_requests,)*
                    _ => ::core::result::Result::Err(::yutani::wire::WlError::INVALID_OPCODE)
                }
            }
            #[doc = "Create a new object that can be tracked by `yutani`"]
            fn into_object(self, id: ::yutani::Id) -> ::yutani::lease::Resident<Self, T, ::yutani::server::Client<T>> {
                ::yutani::lease::Resident::new(id, Self::dispatch, Self::INTERFACE, Self::VERSION, self)
            }
            #[doc = "Create a new object that can be tracked by `yutani`, with a given version"]
            fn into_versioned_object(self, id: ::yutani::Id, version: u32) -> ::core::result::Result<::yutani::lease::Resident<Self, T, ::yutani::server::Client<T>>, ::yutani::wire::WlError<'static>> {
                if version > Self::VERSION {
                    ::core::result::Result::Err(::yutani::wire::WlError::UNSUPPORTED_VERSION)
                } else {
                    ::core::result::Result::Ok(::yutani::lease::Resident::new(id, Self::dispatch, Self::INTERFACE, version, self))
                }
            }
            #(#requests)*
            #(#events)*
        }
        pub mod #mod_ident {
            #(#enums)*
        }
    }
}

pub fn enumeration(enumeration: &Enum) -> TokenStream {
    let ident = Ident::new_raw(&enumeration.name.to_pascal_case(), Span::call_site());
    let since = enumeration.since.map(|since| {
        let since = format!("`Since version {}`", since);
        quote!{
            #[doc = ""]
            #[doc = #since]
        }
    });
    let summary = enumeration.summary.as_ref().map(|summary| quote!{#[doc = #summary]});
    let description = enumeration.description.as_ref().map(|description| quote! {#[doc = #description]});

    let entries = enumeration.entries.iter().map(|entry| {
        let name = if entry.name.starts_with(char::is_numeric) {
            format!("{}_{}", enumeration.name, entry.name).to_shouty_snake_case()
        } else { entry.name.to_shouty_snake_case() };
        let ident = Ident::new_raw(&name, Span::call_site());
        let since = entry.since.map(|since| {
            let since = format!("`Since version {}`", since);
            quote!{
                #[doc = ""]
                #[doc = #since]
            }
        });
        let summary = entry.summary.as_ref().map(|summary| quote!{#[doc = #summary]});
        let description = entry.description.as_ref().map(|description| quote! {#[doc = #description]});
        let value = entry.value;
        quote!{
            #summary
            #since
            #[doc = ""]
            #description
            pub const #ident: Self = Self(#value);
        }
    });
    let entries_debug = enumeration.entries.iter().map(|entry| {
        let name = if entry.name.starts_with(char::is_numeric) {
            format!("{}_{}", enumeration.name, entry.name).to_shouty_snake_case()
        } else { entry.name.to_shouty_snake_case() };
        let value = entry.value;
        quote!{#value => ::core::write!(f, "{}({})", #name, #value)}
    });

    quote!{
        #summary
        #since
        #[doc = ""]
        #description
        #[repr(transparent)]
        pub struct #ident(u32);
        impl #ident {
            #(#entries)*
        }
        impl ::core::convert::From<::core::primitive::u32> for #ident {
            fn from(value: ::core::primitive::u32) -> Self {
                Self(value)
            }
        }
        impl ::core::convert::Into<::core::primitive::u32> for #ident {
            fn into(self) -> ::core::primitive::u32 {
                self.0
            }
        }
        impl ::core::fmt::Debug for #ident {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                match self.0 {
                    #(#entries_debug,)*
                    value => ::core::write!(f, "UNKNOWN({})", value)
                }
            }
        }
    }
}

pub fn request(request: &Request) -> TokenStream {
    let ident = Ident::new_raw(&request.name.to_snake_case(), Span::call_site());
    let since = request.since.map(|since| {
        let since = format!("`Since version {}`", since);
        quote!{
            #[doc = ""]
            #[doc = #since]
        }
    });
    let summary = request.summary.as_ref().map(|summary| quote!{#[doc = #summary]});
    let description = request.description.as_ref().map(|description| quote! {#[doc = #description]});

    let args = request.args.iter().map(|a| {
        let ident = Ident::new_raw(&a.name.to_snake_case(), Span::call_site());
        let ty = a.ty();
        quote!{
            #ident: #ty
        }
    });
    let arg_summaries: Vec<_> = request.args.iter().filter_map(|a| {
        a.summary.as_ref().map(|summary| {
            let summary = format!("\n`{}`: {}", a.name, summary);
            quote!{#[doc = #summary]}
        })
    }).collect();
    let arg_summaries_header = if arg_summaries.is_empty() {
        None
    } else {
        Some(quote!{
            #[doc = ""]
            #[doc = "## Arguments"]
        })
    };

    quote!{
        #summary
        #since
        #[doc = ""]
        #description
        #arg_summaries_header
        #(#arg_summaries)*
        fn #ident(this: ::yutani::lease::Lease<Self>, event_loop: &mut ::yutani::wire::EventLoop<T>, client: &mut ::yutani::server::Client<T> #(, #args)*) -> ::core::result::Result<(), ::yutani::wire::WlError<'static>>;
    }
}

pub fn event(interface: &Interface, event: &Event, opcode: u16) -> TokenStream {
    let name = &interface.name;
    let event_name = &event.name.to_snake_case();
    let ident = Ident::new_raw(event_name, Span::call_site());
    let stream = Ident::new("_stream", Span::call_site());
    let since = event.since.map(|since| {
        let since = format!("`Since version {}`", since);
        quote!{
            #[doc = ""]
            #[doc = #since]
        }
    });
    let summary = event.summary.as_ref().map(|summary| quote!{#[doc = #summary]});
    let description = event.description.as_ref().map(|description| quote! {#[doc = #description]});

    let args = event.args.iter().map(|a| {
        let ident = Ident::new_raw(&a.name.to_snake_case(), Span::call_site());
        let ty = a.send_ty();
        quote!{
            #ident: #ty
        }
    });
    let args_senders = event.args.iter().map(|a| a.sender(&stream));
    let arg_summaries: Vec<_> = event.args.iter().filter_map(|a| {
        a.summary.as_ref().map(|summary| {
            let summary = format!("\n`{}`: {}", a.name, summary);
            quote!{#[doc = #summary]}
        })
    }).collect();
    let arg_summaries_header = if arg_summaries.is_empty() {
        None
    } else {
        Some(quote!{
            #[doc = ""]
            #[doc = "## Arguments"]
        })
    };

    let args_debug_idents = event.args.iter().map(|a| {
        let ident = Ident::new_raw(&a.name.to_snake_case(), Span::call_site());
        quote!{#ident}
    });
    let args_debug_templates = event.args.iter().enumerate().map(|(i, _)| {
        if i == 0 {
            quote!{"{:?}"}
        } else {
            quote!{", {:?}"}
        }
    });

    quote!{
        #summary
        #since
        #[doc = ""]
        #description
        #arg_summaries_header
        #(#arg_summaries)*
        fn #ident(_this: &mut ::yutani::lease::Lease<Self>, _client: &mut ::yutani::server::Client<T> #(, #args)*) -> ::core::result::Result<(), ::yutani::wire::WlError<'static>> {
            #[cfg(debug_assertions)]
            {
                ::std::println!(::std::concat!(" -> ", #name, "@{}.", #event_name, "(", #(#args_debug_templates,)* ")"), _this.id(), #(#args_debug_idents,)*);
            }
            let #stream = _client.stream();
            let _key = #stream.start_message(_this.id(), #opcode);
            #(#args_senders;)*
            #stream.commit(_key)
        }
    }
}