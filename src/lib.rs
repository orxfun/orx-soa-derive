#![doc = include_str!("../README.md")]
#![warn(
    missing_docs,
    clippy::unwrap_in_result,
    clippy::unwrap_used,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::float_cmp,
    clippy::float_cmp_const,
    clippy::missing_panics_doc,
    clippy::todo
)]
#![no_std]

extern crate alloc;

use alloc::string::ToString;
use alloc::vec::Vec;
use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Fields, Type};

fn is_non_send_type(ty: &Type) -> bool {
    match ty {
        Type::Ptr(_) => true,
        Type::Path(path) => {
            let last = path.path.segments.last().map(|seg| seg.ident.to_string());
            matches!(
                last.as_deref(),
                Some("Rc" | "Cell" | "RefCell" | "UnsafeCell" | "Mutex" | "RwLock")
            )
        }
        _ => false,
    }
}

/// Derives struct-of-arrays collection of the provided type.
///
/// # Panics
///
/// - panics when not used on a named struct
#[proc_macro_derive(Soa)]
pub fn derive_soa(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as DeriveInput);

    let name = &input.ident;
    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => &fields.named,
            #[allow(clippy::panic)]
            _ => panic!("Soa only supports structs with named fields"),
        },
        #[allow(clippy::panic)]
        _ => panic!("Soa only supports structs"),
    };

    let field_idents: Vec<_> = fields
        .iter()
        .map(|f| f.ident.as_ref().expect("named-struct").clone())
        .collect();
    let field_types: Vec<_> = fields.iter().map(|f| f.ty.clone()).collect();
    let vec_field_types: Vec<syn::Type> = field_types
        .iter()
        .map(|ty| syn::parse_quote!(Vec<#ty>))
        .collect();
    let include_par_extend = !field_types.iter().any(is_non_send_type);

    let soa_name = format_ident!("{}Soa", name);
    let ptr_name = format_ident!("{}Ptr", name);
    let mut_ptr_name = format_ident!("{}MutPtr", name);
    let iter_name = format_ident!("{}Iter", name);
    let ref_name = format_ident!("{}Ref", name);
    let iter_ref_name = format_ident!("{}IterRef", name);
    let mut_name = format_ident!("{}Mut", name);
    let iter_mut_name = format_ident!("{}IterMut", name);

    let ptr_fields: Vec<_> = field_idents
        .iter()
        .enumerate()
        .map(|(idx, field)| {
            let ty = &field_types[idx];
            quote! { pub #field: *const #ty, }
        })
        .collect();

    let mut_ptr_fields: Vec<_> = field_idents
        .iter()
        .enumerate()
        .map(|(idx, field)| {
            let ty = &field_types[idx];
            quote! { pub #field: *mut #ty, }
        })
        .collect();

    let ptr_add_fields: Vec<_> = field_idents
        .iter()
        .map(|field| quote! { #field: unsafe { self.#field.add(count) }, })
        .collect();

    let ptr_clone_copy_fields: Vec<_> = field_idents
        .iter()
        .map(|field| quote! { #field: self.#field, })
        .collect();

    let ptr_copy_fields: Vec<_> = field_idents
        .iter()
        .map(|field| quote! { unsafe { self.#field.copy_from_nonoverlapping(src.#field, count) }; })
        .collect();

    let vec_field_defs: Vec<_> = field_idents
        .iter()
        .enumerate()
        .map(|(idx, field)| {
            let ty = &field_types[idx];
            quote! { #field: Vec<#ty>, }
        })
        .collect();

    let vec_new_inits: Vec<_> = field_idents
        .iter()
        .map(|field| quote! { #field: Vec::new(), })
        .collect();

    let vec_with_capacity_inits: Vec<_> = field_idents
        .iter()
        .map(|field| quote! { #field: Vec::with_capacity(capacity), })
        .collect();

    let into_inner_values: Vec<_> = field_idents
        .iter()
        .map(|field| quote! { self.#field })
        .collect();

    let as_ptr_assignments: Vec<_> = field_idents
        .iter()
        .map(|field| quote! { #field: self.#field.as_ptr(), })
        .collect();

    let first = &field_idents[0];
    let rest = field_idents.iter().skip(1).collect::<Vec<_>>();
    let get = if field_idents.len() == 1 {
        quote! {
            self.#first.get(index).map(|#first| #ref_name { #first })
        }
    } else {
        quote! {
            self.#first.get(index).map(|#first| {
                #(
                    let #rest = &self.#rest[index];
                )*
                #ref_name { #(#field_idents),* }
            })
        }
    };
    let get_mut = if field_idents.len() == 1 {
        quote! {
            self.#first.get_mut(index).map(|#first| #mut_name { #first })
        }
    } else {
        quote! {
            self.#first.get_mut(index).map(|#first| {
                #(
                    let #rest = &mut self.#rest[index];
                )*
                #mut_name { #(#field_idents),* }
            })
        }
    };

    let as_mut_ptr_assignments: Vec<_> = field_idents
        .iter()
        .map(|field| quote! { #field: self.#field.as_mut_ptr(), })
        .collect();

    let push_fields: Vec<_> = field_idents
        .iter()
        .map(|field| quote! { self.#field.push(item.#field); })
        .collect();

    let reserve_fields: Vec<_> = field_idents
        .iter()
        .map(|field| quote! { self.#field.reserve(additional); })
        .collect();

    let set_len_fields: Vec<_> = field_idents
        .iter()
        .map(|field| quote! { unsafe { self.#field.set_len(new_len) }; })
        .collect();

    let accessors: Vec<_> = field_idents
        .iter()
        .enumerate()
        .map(|(idx, field)| {
            let ty = &field_types[idx];
            quote! {
                pub fn #field(&self) -> &[#ty] {
                    &self.#field
                }
            }
        })
        .collect();

    let mut_accessors: Vec<_> = field_idents
        .iter()
        .enumerate()
        .map(|(idx, field)| {
            let ty = &field_types[idx];
            let method = format_ident!("{}_mut", field);
            quote! {
                pub fn #method(&mut self) -> &mut [#ty] {
                    &mut self.#field
                }
            }
        })
        .collect();

    let extend_fields: Vec<_> = field_idents
        .iter()
        .map(|field| quote! { self.#field.push(x.#field); })
        .collect();

    let iter_fields: Vec<_> = field_idents
        .iter()
        .enumerate()
        .map(|(idx, field)| {
            let ty = &field_types[idx];
            quote! { #field: ::std::vec::IntoIter<#ty>, }
        })
        .collect();

    let iter_ref_fields: Vec<_> = field_idents
        .iter()
        .enumerate()
        .map(|(idx, field)| {
            let ty = &field_types[idx];
            quote! { #field: ::core::slice::Iter<'a, #ty>, }
        })
        .collect();

    let iter_mut_fields: Vec<_> = field_idents
        .iter()
        .enumerate()
        .map(|(idx, field)| {
            let ty = &field_types[idx];
            quote! { #field: ::core::slice::IterMut<'a, #ty>, }
        })
        .collect();

    let iter_values: Vec<_> = field_idents
        .iter()
        .map(|field| quote! { #field: self.#field.into_iter(), })
        .collect();

    let iter_ref_values: Vec<_> = field_idents
        .iter()
        .map(|field| quote! { #field: self.#field.iter(), })
        .collect();

    let iter_mut_values: Vec<_> = field_idents
        .iter()
        .map(|field| quote! { #field: self.#field.iter_mut(), })
        .collect();

    let first = &field_idents[0];
    let rest = field_idents.iter().skip(1).collect::<Vec<_>>();
    let iter_next = if field_idents.len() == 1 {
        quote! {
            self.#first.next().map(|#first| #name { #first })
        }
    } else {
        quote! {
            self.#first.next().map(|#first| {
                #(
                    let #rest = unsafe { self.#rest.next().unwrap_unchecked() };
                )*
                #name { #(#field_idents),* }
            })
        }
    };

    let ref_fields: Vec<_> = field_idents
        .iter()
        .enumerate()
        .map(|(idx, field)| {
            let ty = &field_types[idx];
            quote! { #field: &'a #ty, }
        })
        .collect();

    let mut_ref_fields: Vec<_> = field_idents
        .iter()
        .enumerate()
        .map(|(idx, field)| {
            let ty = &field_types[idx];
            quote! { #field: &'a mut #ty, }
        })
        .collect();

    let iter_ref_next = if field_idents.len() == 1 {
        quote! {
            self.#first.next().map(|#first| #ref_name { #first })
        }
    } else {
        quote! {
            self.#first.next().map(|#first| {
                #(
                    let #rest = unsafe { self.#rest.next().unwrap_unchecked() };
                )*
                #ref_name { #(#field_idents),* }
            })
        }
    };

    let iter_mut_next = if field_idents.len() == 1 {
        quote! {
            self.#first.next().map(|#first| #mut_name { #first })
        }
    } else {
        quote! {
            self.#first.next().map(|#first| {
                #(
                    let #rest = unsafe { self.#rest.next().unwrap_unchecked() };
                )*
                #mut_name { #(#field_idents),* }
            })
        }
    };

    let ordered_thread_values = quote! {
        fn add_ordered_thread_value(collected: &mut Self::OrderedThreadValues, idx: usize, value: #name) {
            collected.values.push(value);
            collected.positions.push(::orx_parallel::collectables::IdxLen { idx, len: 1 });
        }
        fn add_ordered_thread_values(collected: &mut Self::OrderedThreadValues, idx: usize, values: impl IntoIterator<Item = #name>) {
            let len_begin = collected.values.len();
            collected.values.extend(values);
            let len = collected.values.len() - len_begin;
            if len > 0 {
                collected.positions.push(::orx_parallel::collectables::IdxLen { idx, len });
            }
        }
        fn add_ordered_thread_optionals(collected: &mut Self::OrderedThreadValues, idx: usize, values: impl IntoIterator<Item = Option<#name>>) -> Option<()> {
            let len_begin = collected.values.len();
            for value in values {
                collected.values.push(value?);
            }
            let len = collected.values.len() - len_begin;
            if len > 0 {
                collected.positions.push(::orx_parallel::collectables::IdxLen { idx, len });
            }
            Some(())
        }
        fn add_ordered_thread_fallibles<E>(collected: &mut Self::OrderedThreadValues, idx: usize, values: impl IntoIterator<Item = Result<#name, E>>) -> Result<(), E> {
            let len_begin = collected.values.len();
            for value in values {
                collected.values.push(value?);
            }
            let len = collected.values.len() - len_begin;
            if len > 0 {
                collected.positions.push(::orx_parallel::collectables::IdxLen { idx, len });
            }
            Ok(())
        }
        fn add_one(&mut self, value: #name) { self.push(value); }
        fn extend_merge_infallibles(&mut self, results: Vec<Self::ThreadValues>) {
            let collected_len: usize = results.iter().map(|x| x.len()).sum();
            self.reserve(collected_len);
            for result in results {
                self.extend(result);
            }
        }
        fn extend_merge_ordered_infallibles(&mut self, mut results: Vec<Self::OrderedThreadValues>) {
            let collected_len: usize = results.iter().map(|x| x.values.len()).sum();
            self.reserve(collected_len);
            let initial_len = self.len();
            let total_len = initial_len + collected_len;
            let mut queue = ::orx_priority_queue::BinaryHeap::with_capacity(results.len());
            let mut pos_indices = ::std::vec![0; results.len()];
            for (t, vec) in results.iter().enumerate() {
                if let Some(pos) = vec.positions.first() {
                    let node = ::orx_parallel::collectables::ThBegLen::new(t, 0, pos.len);
                    queue.push(node, pos.idx);
                }
            }
            let mut curr_t = queue.pop_node();
            let mut ptr_dst = unsafe { self.as_mut_ptr().add(initial_len) };
            while let Some(::orx_parallel::collectables::ThBegLen { th, beg, len }) = curr_t {
                let ptr_src = unsafe { results[th].values.as_ptr().add(beg) };
                unsafe { ptr_dst.copy_from_nonoverlapping(ptr_src, len) };
                pos_indices[th] += 1;
                curr_t = match results[th].positions.get(pos_indices[th]) {
                    Some(pos) => {
                        let beg = beg + len;
                        let node = ::orx_parallel::collectables::ThBegLen::new(th, beg, pos.len);
                        Some(queue.push_then_pop(node, pos.idx).0)
                    }
                    None => queue.pop_node(),
                };
                ptr_dst = unsafe { ptr_dst.add(len) };
            }
            for vec in results.iter_mut() {
                unsafe { vec.values.set_len(0) };
            }
            unsafe { self.set_len(total_len) };
        }
    };

    let par_extend_impl = if include_par_extend {
        quote! {
            use ::orx_priority_queue::PriorityQueue as _;
            impl ::orx_parallel::collectables::ParExtendCore<#name> for #soa_name {
                type ThreadValues = Self;
                type OrderedThreadValues = ::orx_parallel::collectables::ColAndPos<Self>;
                fn new_thread_values() -> Self::ThreadValues { Default::default() }
                fn new_ordered_thread_values() -> Self::OrderedThreadValues { Default::default() }
                fn add_thread_value(collected: &mut Self::ThreadValues, value: #name) { collected.push(value); }
                fn add_thread_values(collected: &mut Self::ThreadValues, values: impl IntoIterator<Item = #name>) { collected.extend(values); }
                #ordered_thread_values
            }
        }
    } else {
        quote! {}
    };

    let expanded = quote! {
        pub struct #ptr_name {
            #(#ptr_fields)*
        }

        impl Clone for #ptr_name {
            fn clone(&self) -> Self {
                Self {
                    #(#ptr_clone_copy_fields)*
                }
            }
        }

        impl Copy for #ptr_name {}

        impl #ptr_name {
            pub unsafe fn add(self, count: usize) -> Self {
                Self {
                    #(#ptr_add_fields)*
                }
            }
        }

        pub struct #mut_ptr_name {
            #(#mut_ptr_fields)*
        }

        impl Clone for #mut_ptr_name {
            fn clone(&self) -> Self {
                Self {
                    #(#ptr_clone_copy_fields)*
                }
            }
        }

        impl Copy for #mut_ptr_name {}

        impl #mut_ptr_name {
            pub unsafe fn add(self, count: usize) -> Self {
                Self {
                    #(#ptr_add_fields)*
                }
            }

            pub unsafe fn copy_from_nonoverlapping(self, src: #ptr_name, count: usize) {
                #(#ptr_copy_fields)*
            }
        }

        pub struct #soa_name {
            #(#vec_field_defs)*
        }

        impl #soa_name {
            pub fn new() -> Self {
                Self {
                    #(#vec_new_inits)*
                }
            }

            pub fn with_capacity(capacity: usize) -> Self {
                Self {
                    #(#vec_with_capacity_inits)*
                }
            }

            pub fn len(&self) -> usize {
                self.#first.len()
            }

            pub fn is_empty(&self) -> bool {
                self.#first.is_empty()
            }

            pub fn into_inner(self) -> (#(#vec_field_types, )*) {
                (#(#into_inner_values, )*)
            }

            #(#accessors)*
            #(#mut_accessors)*

            pub fn as_ptr(&self) -> #ptr_name {
                #ptr_name {
                    #(#as_ptr_assignments)*
                }
            }

            pub fn get(&self, index:usize) -> Option<#ref_name> {
                #get
            }

            pub fn get_mut(&mut self, index:usize) -> Option<#mut_name> {
                #get_mut
            }

            pub fn push(&mut self, item: #name) {
                #(#push_fields)*
            }

            pub fn as_mut_ptr(&mut self) -> #mut_ptr_name {
                #mut_ptr_name {
                    #(#as_mut_ptr_assignments)*
                }
            }

            pub fn reserve(&mut self, additional: usize) {
                #(#reserve_fields)*
            }

            pub unsafe fn set_len(&mut self, new_len: usize) {
                #(#set_len_fields)*
            }
        }

        impl Default for #soa_name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl From<#soa_name> for (#(#vec_field_types, )*) {
            fn from(value: #soa_name) -> Self {
                (#(value.#field_idents, )*)
            }
        }

        impl Extend<#name> for #soa_name {
            fn extend<I: IntoIterator<Item = #name>>(&mut self, iter: I) {
                for x in iter {
                    #(#extend_fields)*
                }
            }
        }

        pub struct #iter_name {
            #(#iter_fields)*
        }

        impl Iterator for #iter_name {
            type Item = #name;

            fn next(&mut self) -> Option<Self::Item> {
                #iter_next
            }
        }

        impl IntoIterator for #soa_name {
            type Item = #name;
            type IntoIter = #iter_name;

            fn into_iter(self) -> Self::IntoIter {
                #iter_name {
                    #(#iter_values)*
                }
            }
        }

        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        pub struct #ref_name<'a> {
            #(#ref_fields)*
        }

        pub struct #iter_ref_name<'a> {
            #(#iter_ref_fields)*
        }

        impl<'a> Iterator for #iter_ref_name<'a> {
            type Item = #ref_name<'a>;

            fn next(&mut self) -> Option<Self::Item> {
                #iter_ref_next
            }
        }

        impl<'a> IntoIterator for &'a #soa_name {
            type Item = #ref_name<'a>;
            type IntoIter = #iter_ref_name<'a>;

            fn into_iter(self) -> Self::IntoIter {
                #iter_ref_name {
                    #(#iter_ref_values)*
                }
            }
        }

        #[derive(PartialEq, Eq, Debug)]
        pub struct #mut_name<'a> {
            #(#mut_ref_fields)*
        }

        pub struct #iter_mut_name<'a> {
            #(#iter_mut_fields)*
        }

        impl<'a> Iterator for #iter_mut_name<'a> {
            type Item = #mut_name<'a>;

            fn next(&mut self) -> Option<Self::Item> {
                #iter_mut_next
            }
        }

        impl<'a> IntoIterator for &'a mut #soa_name {
            type Item = #mut_name<'a>;
            type IntoIter = #iter_mut_name<'a>;

            fn into_iter(self) -> Self::IntoIter {
                #iter_mut_name {
                    #(#iter_mut_values)*
                }
            }
        }

        #par_extend_impl
    };

    expanded.into()
}
