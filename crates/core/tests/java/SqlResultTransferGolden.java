package com.tangluobo.tomato.module.connect.view;
import javafx.application.*;
import javafx.stage.Stage;
import javafx.scene.control.*;
import javafx.collections.FXCollections;
import com.google.gson.*;
import java.nio.file.*;
import java.util.*;
/** Uses pure original table formatting methods; never touches the OS clipboard. */
public class SqlResultTransferGolden {
 public static void main(String[] args){Application.launch(Probe.class,args);}
 public static class Probe extends Application {
  public void start(Stage stage)throws Exception {
   var columns=List.of("USER_ID","alreadyCamel","ÉTAT_NAME","中文-字段","I_İ_ß","tab\theader");
   var rows=List.of(Arrays.asList("1",null,"NULL","中文\n下一行","a\tb\rc","\"\\\b\f\u0000\u001f\u2028😀"),Arrays.asList("2","","résumé","ſ ΣΟΣ","\n\r","literal NULL"),Arrays.asList("3","a'b","ß","中文","中\t文",null));
   var comments=new LinkedHashMap<String,String>();comments.put("user_id","用户编号");comments.put("alreadyCamel","精确注释");comments.put("ALREADYCAMEL","其他大小写注释");comments.put("ÉTAT_NAME","\u2003");comments.put("中文-字段","两行\n注释");comments.put("I_İ_ß","\u00a0");
   var output=new JsonObject();var cases=new JsonArray();var gson=new GsonBuilder().serializeNulls().create();
   var tsv=TableCellContextMenuUtils.class.getDeclaredMethod("selectedTsv",TableView.class,int.class,boolean.class,boolean.class);tsv.setAccessible(true);
   var all=TableCellContextMenuUtils.class.getDeclaredMethod("allTsv",TableView.class,int.class);all.setAccessible(true);
   for(int sample=0;sample<6;sample++){
    var table=new TableView<javafx.collections.ObservableList<String>>();var selector=new TableColumn<javafx.collections.ObservableList<String>,String>("");table.getColumns().add(selector);
    for(int col=0;col<columns.size();col++){var column=new TableColumn<javafx.collections.ObservableList<String>,String>(columns.get(col));column.setUserData(col);table.getColumns().add(column);}
    for(var row:rows)table.getItems().add(FXCollections.observableArrayList(row));table.getSelectionModel().setCellSelectionEnabled(true);table.getSelectionModel().setSelectionMode(SelectionMode.MULTIPLE);
    if(sample==1)table.getSelectionModel().select(0,selector);
    if(sample>=2){table.getSelectionModel().select(0,table.getColumns().get(1));table.getSelectionModel().select(2,table.getColumns().get(3));}
    if(sample==3){table.getSelectionModel().clearSelection();table.getSelectionModel().select(1,table.getColumns().get(2));table.getSelectionModel().select(1,table.getColumns().get(6));}
    if(sample==4){var reordered=new ArrayList<>(table.getColumns());Collections.swap(reordered,1,4);table.getColumns().setAll(reordered);}
    if(sample==5)table.getColumns().get(3).setVisible(false);
    var item=new JsonObject();var labels=new ArrayList<String>();var identities=new ArrayList<Integer>();for(int i=1;i<table.getColumns().size();i++){var column=table.getColumns().get(i);labels.add(column.getText());identities.add((Integer)column.getUserData());}
    var selection=new JsonArray();for(var position:table.getSelectionModel().getSelectedCells()){int column=table.getColumns().indexOf(position.getTableColumn())-1;if(column<0)continue;selection.add(gson.toJsonTree(new int[]{position.getRow(),column}));}
    item.addProperty("sample",sample);item.add("columns",gson.toJsonTree(labels));item.add("identities",gson.toJsonTree(identities));item.add("selected",selection);item.addProperty("json",TableCellContextMenuUtils.selectedJson(table,1,false));item.addProperty("camelJson",TableCellContextMenuUtils.selectedJson(table,1,true));item.addProperty("comments",TableCellContextMenuUtils.selectedCommentValues(table,1,comments));item.addProperty("data",(String)tsv.invoke(null,table,1,false,true));item.addProperty("headers",(String)tsv.invoke(null,table,1,true,false));item.addProperty("headersData",(String)tsv.invoke(null,table,1,true,true));item.addProperty("all",(String)all.invoke(null,table,1));cases.add(item);
   }
   output.add("rows",gson.toJsonTree(rows));output.add("comments",gson.toJsonTree(comments));output.add("cases",cases);var camel=new JsonArray();
   for(var input:new String[]{null,"","USER_ID","alreadyCamel","HTTPServer","URL","User Name","_A__B_","---","   ","a\tB\nC","中文_字段","中文","ÉTAT_NAME","İD_VALUE","I_İ_ß","ß_NAME","ẞ_NAME","ΟΣ_ΣΟΣ","a_ΐ","a_ﬀ","ſ_TEST","𐐀_NAME","😀_VALUE","a\u2003B","a\u00a0B","\u2003","123_ABC","A-B","A\r\nB","a_$VALUE"}){var item=new JsonObject();item.add("input",gson.toJsonTree(input));item.addProperty("output",TableCellContextMenuUtils.toLowerCamelCase(input));camel.add(item);}output.add("camel",camel);
   var path=Path.of(getParameters().getRaw().get(0));Files.createDirectories(path.getParent());Files.writeString(path,new GsonBuilder().serializeNulls().setPrettyPrinting().disableHtmlEscaping().create().toJson(output));Platform.exit();System.exit(0);
  }
 }
}
