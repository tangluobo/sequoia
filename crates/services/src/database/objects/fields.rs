//! Shared projections feed complete catalog reads and quiet background preload.
use super::*;
use tomato_core::database_objects::field_bytes;
pub(super) mod load;

pub(super) async fn read(
    session: &mut AnyConnection,
    kind: &str,
    schema: &str,
    catalog: &mut Catalog,
    retained: &mut usize,
) -> Result<()> {
    let values = load::read(
        session,
        kind,
        schema,
        catalog,
        BUDGET.saturating_sub(*retained),
        false,
    )
    .await?;
    let mut names: Vec<_> = (0..catalog.objects.len()).collect();
    names.sort_unstable_by(|&a, &b| {
        catalog.objects[a]
            .table
            .name
            .cmp(&catalog.objects[b].table.name)
    });
    let scratch = names.capacity() * std::mem::size_of::<usize>();
    for table in values {
        let Ok(at) =
            names.binary_search_by(|&index| catalog.objects[index].table.name.cmp(&table.name))
        else {
            continue;
        };
        let target = &mut catalog.objects[names[at]].fields;
        *retained = retained
            .saturating_sub(field_bytes(target))
            .saturating_add(field_bytes(&table.columns));
        ensure!(
            retained.saturating_add(scratch) <= BUDGET,
            "对象目录超过 8 MiB，请缩小数据库范围"
        );
        *target = table.columns;
    }
    Ok(())
}
