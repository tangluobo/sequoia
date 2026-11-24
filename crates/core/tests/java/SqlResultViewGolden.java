package com.tangluobo.tomato.module.connect.view;
import javafx.application.*;
import javafx.stage.Stage;
import javafx.scene.*;
import javafx.scene.control.*;
import javafx.collections.*;
import com.google.gson.*;
import com.tangluobo.tomato.module.connect.TableRowData;
import java.nio.file.*;
import java.util.*;
/** Original TableView sort/filter/selection transitions; no network or clipboard. */
public class SqlResultViewGolden {
 public static void main(String[] args){Application.launch(Probe.class,args);}
 public static class Probe extends Application {
  @SuppressWarnings({"unchecked","rawtypes"})
  public void start(Stage stage)throws Exception {
   var view=new SqlEditorView(List.of(),null,null);var result=new TableRowData();result.setColumnNames(List.of("value","amount","identity"));
   var values=Arrays.asList("b","a","b",null,"😀","\ue000");var amounts=Arrays.asList("10","2","2.00","-1",null,"abc");for(int i=0;i<values.size();i++)result.getRows().add(FXCollections.observableArrayList(Arrays.asList(values.get(i),amounts.get(i),"r"+i)));
   var create=SqlEditorView.class.getDeclaredMethod("createTableView",TableRowData.class);create.setAccessible(true);var table=(TableView<ObservableList<String>>)find((Node)create.invoke(view,result));
   var utility=TableCellContextMenuUtils.class;var op=Class.forName(utility.getName()+"$FilterOperator");var condition=Class.forName(utility.getName()+"$FilterCondition");var constructor=condition.getDeclaredConstructor(op,String.class);constructor.setAccessible(true);var put=utility.getDeclaredMethod("putFilter",TableView.class,int.class,condition);put.setAccessible(true);var get=utility.getDeclaredMethod("getFilters",TableView.class,boolean.class);get.setAccessible(true);var apply=utility.getDeclaredMethod("applyFilters",TableView.class);apply.setAccessible(true);var clear=utility.getDeclaredMethod("clearAllFilters",TableView.class);clear.setAccessible(true);
   var cases=new JsonArray();cases.add(record("initial",table));var text=table.getColumns().get(1);var number=table.getColumns().get(2);table.getSelectionModel().clearAndSelect(0,text);cases.add(record("selected-r0",table));
   table.getSortOrder().setAll(List.of(text));table.sort();cases.add(record("value-ascending",table));
   put.invoke(null,table,0,constructor.newInstance(Enum.valueOf((Class)op,"LIKE"),"b"));cases.add(record("like-b",table));
   number.setSortType(TableColumn.SortType.DESCENDING);table.getSortOrder().setAll(List.of(number));table.sort();cases.add(record("amount-descending",table));
   put.invoke(null,table,1,constructor.newInstance(Enum.valueOf((Class)op,"LESS"),"5"));cases.add(record("and-less-five",table));
   ((Map)get.invoke(null,table,false)).remove(0);apply.invoke(null,table);cases.add(record("remove-value-filter",table));
   table.getSortOrder().clear();table.sort();cases.add(record("clear-sort-filtered",table));clear.invoke(null,table);cases.add(record("clear-all-filters",table));
   number.setSortType(TableColumn.SortType.ASCENDING);table.getSortOrder().setAll(List.of(number));table.sort();cases.add(record("amount-ascending",table));
   put.invoke(null,table,0,constructor.newInstance(Enum.valueOf((Class)op,"EQUAL"),null));cases.add(record("equals-null",table));
   TableCellContextMenuUtils.removeRows(table,List.of(table.getItems().getFirst()));cases.add(record("delete-visible-null-row",table));TableCellContextMenuUtils.clearSortAndFilter(table);cases.add(record("clear-after-delete",table));
   text.setVisible(false);var reordered=new ArrayList<>(table.getColumns());Collections.swap(reordered,1,3);table.getColumns().setAll(reordered);cases.add(record("hide-and-reorder",table));
   put.invoke(null,table,1,constructor.newInstance(Enum.valueOf((Class)op,"EQUAL"),"2"));cases.add(record("filter-physical-amount-after-reorder",table));
   number.setSortType(TableColumn.SortType.DESCENDING);table.getSortOrder().setAll(List.of(number));table.sort();cases.add(record("sort-active-filtered",table));clear.invoke(null,table);cases.add(record("clear-filter-with-active-sort",table));
   text.setVisible(true);text.setSortType(TableColumn.SortType.ASCENDING);table.getSortOrder().setAll(List.of(text));table.sort();
   put.invoke(null,table,0,constructor.newInstance(Enum.valueOf((Class)op,"LIKE"),"b"));cases.add(record("tie-base",table));
   number.setSortType(TableColumn.SortType.DESCENDING);table.getSortOrder().setAll(List.of(number));table.sort();cases.add(record("tie-reverse",table));
   table.getSortOrder().setAll(List.of(text));table.sort();cases.add(record("tie-filtered-back-to-value",table));
   clear.invoke(null,table);for(var column:table.getColumns())if(column.getUserData() instanceof Integer index&&index>=0)column.setVisible(false);
   table.getSelectionModel().clearAndSelect(0);cases.add(record("all-hidden-row-selected",table));
   var path=Path.of(getParameters().getRaw().get(0));Files.createDirectories(path.getParent());Files.writeString(path,new GsonBuilder().serializeNulls().setPrettyPrinting().disableHtmlEscaping().create().toJson(cases));Platform.exit();System.exit(0);
  }
 }
 static TableView<?> find(Node node){if(node instanceof TableView<?> table&&table.getColumns().size()>1)return table;if(node instanceof Parent parent)for(var child:parent.getChildrenUnmodifiable()){var found=find(child);if(found!=null)return found;}return null;}
 static JsonObject record(String name,TableView<ObservableList<String>> table){var item=new JsonObject();item.addProperty("name",name);var gson=new GsonBuilder().serializeNulls().create();item.add("rows",gson.toJsonTree(table.getItems()));var selected=new JsonArray();for(var row:table.getSelectionModel().getSelectedItems())selected.add(row.get(2));item.add("selectedRows",selected);var sorts=new JsonArray();for(var column:table.getSortOrder()){var sort=new JsonObject();sort.addProperty("column",column.getText());sort.addProperty("type",column.getSortType().toString());sorts.add(sort);}item.add("sorts",sorts);var columns=new JsonArray();for(var column:table.getColumns()){if(!(column.getUserData() instanceof Integer index)||index<0)continue;var value=new JsonObject();value.addProperty("label",column.getText());value.addProperty("identity",index);value.addProperty("visible",column.isVisible());columns.add(value);}item.add("columns",columns);return item;}
}
