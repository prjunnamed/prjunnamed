use std::{
    cell::{Ref, RefCell},
    fmt::{Debug, Display},
};

use prjunnamed_netlist::{CellRef, Const, Design, Net, Trit, Value};

pub trait NetOrValue: Sized + Clone + Debug + Display {
    fn len(&self) -> usize;
    fn iter(&self) -> impl Iterator<Item = Net>;
    fn as_value(&self) -> Value {
        Value::from_iter(self.iter())
    }
    fn as_const(&self) -> Option<Const>;
    fn try_from(other: impl NetOrValue) -> Option<Self>;

    #[must_use]
    fn accumulate(capture: &mut Option<Self>, net: Net) -> bool;
}

impl NetOrValue for Net {
    fn len(&self) -> usize {
        1
    }

    fn as_const(&self) -> Option<Const> {
        Net::as_const(*self).map(|trit| Const::from(trit))
    }

    fn iter(&self) -> impl Iterator<Item = Net> {
        std::iter::once(*self)
    }

    fn try_from(other: impl NetOrValue) -> Option<Self> {
        if other.len() == 1 {
            other.iter().next()
        } else {
            None
        }
    }

    fn accumulate(capture: &mut Option<Self>, net: Net) -> bool {
        match capture {
            Some(captured_net) if *captured_net == net => return true,
            Some(_) => return false,
            None => *capture = Some(net),
        }
        true
    }
}

impl NetOrValue for Value {
    fn len(&self) -> usize {
        Value::len(self)
    }

    fn iter(&self) -> impl Iterator<Item = Net> {
        self.iter()
    }

    fn as_const(&self) -> Option<Const> {
        Value::as_const(self)
    }

    fn try_from(other: impl NetOrValue) -> Option<Self> {
        Some(Value::from_iter(other.iter()))
    }

    fn accumulate(capture: &mut Option<Self>, net: Net) -> bool {
        match capture {
            Some(value) => *value = value.concat(net),
            None => *capture = Some(Value::from(net)),
        }
        true
    }
}

// Used to interpose accesses to the design in order to track which nets were examined.
pub trait DesignDyn {
    fn find_cell(&self, net: Net) -> Result<(CellRef, usize), Trit>;

    fn inner(&self) -> &Design;
}

impl DesignDyn for Design {
    #[inline]
    fn find_cell(&self, net: Net) -> Result<(CellRef, usize), Trit> {
        Design::find_cell(self, net)
    }

    fn inner(&self) -> &Design {
        self
    }
}

pub struct CellCollector<'a> {
    inner: &'a dyn DesignDyn,
    cells: RefCell<Vec<CellRef<'a>>>,
}

impl<'a> CellCollector<'a> {
    pub fn new(design: &'a dyn DesignDyn) -> Self {
        Self { inner: design, cells: RefCell::new(Vec::new()) }
    }

    pub fn cells(&self) -> Ref<[CellRef<'a>]> {
        Ref::map(self.cells.borrow(), |cells| &cells[..])
    }

    pub fn clear(&self) {
        self.cells.borrow_mut().clear();
    }
}

impl DesignDyn for CellCollector<'_> {
    #[inline]
    fn find_cell(&self, net: Net) -> Result<(CellRef, usize), Trit> {
        match self.inner.find_cell(net) {
            Ok((cell_ref, offset)) => {
                self.cells.borrow_mut().push(cell_ref);
                Ok((cell_ref, offset))
            }
            Err(trit) => Err(trit),
        }
    }

    fn inner(&self) -> &Design {
        self.inner.inner()
    }
}
