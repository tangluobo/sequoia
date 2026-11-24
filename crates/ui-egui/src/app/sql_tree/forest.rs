//! SQL sidebar identity: connection -> actual database -> namespace -> object folders.
//! The routed connection is retained once per opened database, never per tree row.
use std::{collections::HashMap,sync::Arc};
use tomato_core::{Connection,workbench::TableInfo};
#[derive(Clone,Debug,Hash,PartialEq,Eq)]
pub(super) struct Scope{
    pub database:String,
    pub namespace:String,
}
impl Scope{
    pub fn mysql(database:&str)->Self{Self{database:database.into(),namespace:database.into()}}
    pub fn postgres(database:&str,namespace:&str)->Self{Self{database:database.into(),namespace:namespace.into()}}
    pub fn id(&self,connection:&str)->String{
        format!("sql-scope:{}:{}{}:{}{}:{}",connection.len(),connection,self.database.len(),self.database,self.namespace.len(),self.namespace)
    }
}
#[derive(Default)]
pub(super) struct Catalog{
    pub names:Arc<Vec<String>>,
    pub databases:HashMap<String,Database>,
    pub loading:bool,
    pub error:Option<String>,
    pub generation:u64,
}
pub(super) struct Database{
    pub connection:Arc<Connection>,
    pub namespaces:Arc<Vec<String>>,
    pub opened:bool,
    pub loading:bool,
    pub generation:u64,
    pub error:Option<String>,
    pub scopes:HashMap<String,Namespace>,
}
pub(super) struct Namespace{
    pub tables:Arc<Vec<TableInfo>>,
    pub opened:bool,
    pub loading:bool,
    pub generation:u64,
    pub error:Option<String>,
}
impl Default for Namespace{
    fn default()->Self{Self{tables:Arc::new(Vec::new()),opened:false,loading:false,generation:0,error:None}}
}
impl Catalog{
    pub fn open(&mut self,base:&Connection,database:&str,generation:u64)->&mut Database{
        self.databases.entry(database.into()).or_insert_with(||{
            let mut connection=base.clone();connection.set("database",database.to_owned());
            Database{connection:Arc::new(connection),namespaces:Arc::new(Vec::new()),opened:true,loading:false,generation,error:None,scopes:HashMap::new()}
        })
    }
    pub fn close(&mut self,database:&str){self.databases.remove(database);}
    pub fn replace(&mut self,names:Vec<String>){
        self.databases.retain(|name,_|names.contains(name));self.names=Arc::new(names);self.loading=false;self.error=None;
    }
    pub fn scope(&self,scope:&Scope)->Option<&Namespace>{self.databases.get(&scope.database)?.scopes.get(&scope.namespace)}
    pub fn scope_mut(&mut self,scope:&Scope)->Option<&mut Namespace>{self.databases.get_mut(&scope.database)?.scopes.get_mut(&scope.namespace)}
    pub fn retained_bytes(&self)->usize{
        self.databases.iter().fold(self.names.iter().fold(self.names.capacity()*std::mem::size_of::<String>(),|sum,name|sum.saturating_add(name.capacity())),|sum,(name,database)|{
            database.scopes.iter().fold(sum.saturating_add(name.capacity()).saturating_add(database.namespaces.iter().map(String::capacity).sum::<usize>()),|sum,(namespace,value)|{
                value.tables.iter().fold(sum.saturating_add(namespace.capacity()).saturating_add(value.tables.capacity()*std::mem::size_of::<TableInfo>()),|sum,table|sum.saturating_add(table.table.schema.capacity()).saturating_add(table.table.name.capacity()))
            })
        })
    }
}
