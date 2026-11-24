import javafx.application.*;
import javafx.animation.PauseTransition;
import javafx.scene.*;
import javafx.scene.control.*;
import javafx.scene.layout.StackPane;
import javafx.stage.Stage;
import javafx.util.Duration;
import javafx.collections.FXCollections;
import com.google.gson.*;
import com.tangluobo.tomato.module.connect.TableRowData;
import com.tangluobo.tomato.module.connect.view.SqlEditorView;
import java.nio.file.*;
/** Executes sorting on the original result TableView; no network or clipboard. */
public class SqlResultSortGolden {
 public static void main(String[] args){Application.launch(Probe.class,args);}
 public static class Probe extends Application {
  public void start(Stage stage)throws Exception {
   var view=new SqlEditorView(java.util.List.of(),null,null);var data=new TableRowData();data.setColumnNames(java.util.List.of("value","identity"));
   var values=java.util.Arrays.asList("b","a","b",null,"😀","\ue000");
   for(int i=0;i<values.size();i++)data.getRows().add(FXCollections.observableArrayList(java.util.Arrays.asList(values.get(i),"r"+i)));
   var method=SqlEditorView.class.getDeclaredMethod("createTableView",TableRowData.class);method.setAccessible(true);var node=(Node)method.invoke(view,data);stage.setScene(new Scene(new StackPane(node),700,450));stage.setOpacity(.01);stage.show();
   var wait=new PauseTransition(Duration.seconds(1));wait.setOnFinished(event->{try{
    @SuppressWarnings("unchecked") var table=(TableView<javafx.collections.ObservableList<String>>)find(node);var column=table.getColumns().get(1);
    table.getSelectionModel().clearAndSelect(0,column);var cases=new JsonArray();cases.add(record("original",table));
    table.getSortOrder().setAll(java.util.List.of(column));table.sort();cases.add(record("ascending",table));
    column.setSortType(TableColumn.SortType.DESCENDING);table.sort();cases.add(record("descending",table));
    table.getSortOrder().clear();table.sort();cases.add(record("cleared",table));
    var output=Path.of(getParameters().getRaw().get(0));Files.createDirectories(output.getParent());Files.writeString(output,new GsonBuilder().setPrettyPrinting().disableHtmlEscaping().create().toJson(cases));stage.close();Platform.exit();System.exit(0);
   }catch(Throwable error){error.printStackTrace();System.exit(1);}});wait.play();
  }
 }
 static TableView<?> find(Node node){if(node instanceof TableView<?> table&&table.getColumns().size()>1)return table;if(node instanceof Parent parent)for(var child:parent.getChildrenUnmodifiable()){var found=find(child);if(found!=null)return found;}return null;}
 static JsonObject record(String name,TableView<javafx.collections.ObservableList<String>> table){var item=new JsonObject();item.addProperty("name",name);item.add("rows",new Gson().toJsonTree(table.getItems()));item.add("selectedRows",new Gson().toJsonTree(table.getSelectionModel().getSelectedIndices()));return item;}
}
