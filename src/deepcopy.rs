use crate::alloc::Alloc;
use crate::vm::{Value, Closure, Object};

fn deepcopy(src_val: Value, dst_alloc: &mut Alloc) -> Value
{
    if !src_val.is_heap() {
        return src_val;
    }


    // NOTE: should this be a hash of values?
    // We technically only need this for pointer values
    // So we could implement our own hash trait, potentially...
    // Or hash based on the raw bytes

    //
    // Mapping from old to new addresses
    //let dst_map: HashMap<*> = HashSet::new();

    // Stack of values to visit
    let mut stack: Vec<Value> = Vec::new();

    stack.push(src_val);

    // We need to keep a mapping from source to dst...
    // We'll use that to translate src_val

    while stack.len() > 0 {





    }



    todo!()
}

#[cfg(test)]
mod tests
{
    use super::*;


    #[test]
    fn copy_atoms()
    {




    }



}
