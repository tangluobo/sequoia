import javafx.application.*;
import javafx.animation.PauseTransition;
import javafx.scene.*;
import javafx.scene.control.*;
import javafx.stage.Stage;
import javafx.util.Duration;
import javafx.embed.swing.SwingFXUtils;
import javafx.collections.FXCollections;
import com.google.gson.*;
import com.tangluobo.tomato.module.connect.*;
import com.tangluobo.tomato.module.connect.view.TableDataView;
import java.nio.file.*;
import java.util.*;
import javax.imageio.ImageIO;
/** The unchanged original open-table view with a synthetic page. The scheduled
 * loader is blocked before its first run; this fixture never creates a JDBC connection. */
public class CellInteractionOracle {
 static Object get(Object view,String name)throws Exception{var f=TableDataView.class.getDeclaredField(name);f.setAccessible(true);return f.get(view);}
 static void set(Object view,String name,Object value)throws Exception{var f=TableDataView.class.getDeclaredField(name);f.setAccessible(true);f.set(view,value);}
 public static void main(String[] args){Application.launch(Probe.class,args);}
 public static class Probe extends Application {
  @SuppressWarnings({"unchecked","rawtypes"})
  public void start(Stage stage)throws Exception {
   var singleton=GlobalConfig.class.getDeclaredField("instance");singleton.setAccessible(true);singleton.set(null,new GlobalConfig());
   var config=new ConnectionConfig();config.setType(ConnectType.MYSQL);config.setId("open-table-inert-oracle");
   var view=new TableDataView(config,"sample_db","items");set(view,"isLoading",true);
   stage.setScene(new Scene(view,896,650));stage.setOpacity(.01);stage.show();
   var wait=new PauseTransition(Duration.seconds(1));wait.setOnFinished(event->{try{
    var data=new TableRowData();data.setColumnNames(List.of("missing","empty","literal","中文字段"));data.setColumnTypes(Collections.nCopies(4,java.sql.Types.VARCHAR));
    for(int i=0;i<1000;i++)data.getRows().add(FXCollections.observableArrayList(Arrays.asList(null,"","NULL","第 "+i+" 行")));
    var update=TableDataView.class.getDeclaredMethod("updateTableView",TableRowData.class);update.setAccessible(true);update.invoke(view,data);
    set(view,"currentPage",1);set(view,"totalCount",2000L);set(view,"totalPages",2);set(view,"isLoading",false);
    var status=TableDataView.class.getDeclaredMethod("updateStatusBar");status.setAccessible(true);status.invoke(view);
    ((Label)get(view,"sqlStatusLabel")).setText("SELECT * FROM `items` LIMIT 1000 OFFSET 0");
    if(getParameters().getRaw().size()>1){var table=(TableView)get(view,"tableView");table.getSelectionModel().select(1,(TableColumn)table.getColumns().get(3));}

    var table=(TableView<javafx.collections.ObservableList<String>>)get(view,"tableView");
    for(int c=1;c<table.getColumns().size();c++){var col=table.getColumns().get(c);int width=c==4?160:80;col.setMinWidth(width);col.setMaxWidth(width);col.setPrefWidth(width);}
    view.applyCss();view.layout();
    TableCell target=null;
    for(var node:table.lookupAll(".table-cell"))if(node instanceof TableCell cell && cell.getIndex()==1 && cell.getTableColumn()==table.getColumns().get(4))target=cell;
    if(target==null)throw new AssertionError("Missing owned cell");
    var journal=new JsonObject();
    javafx.event.Event.fireEvent(target,new javafx.scene.input.MouseEvent(javafx.scene.input.MouseEvent.MOUSE_ENTERED,10,10,10,10,javafx.scene.input.MouseButton.NONE,0,false,false,false,false,false,false,false,false,false,true,new javafx.scene.input.PickResult(target,new javafx.geometry.Point3D(10,10,0),1)));
    journal.addProperty("hoverStartsEdit",table.getEditingCell()!=null);
    click(target,1);journal.addProperty("firstClickEdits",table.getEditingCell()!=null);
    click(target,1);journal.addProperty("selectedClickEdits",table.getEditingCell()!=null);
    view.applyCss();view.layout();
    var input=(TextField)target.lookup(".text-field");journal.addProperty("inlineTextField",input!=null);
    journal.addProperty("nativeWindows",javafx.stage.Window.getWindows().size());
    Files.writeString(Path.of(getParameters().getRaw().get(0)+".events.json"),new GsonBuilder().setPrettyPrinting().create().toJson(journal));
    if(input==null)throw new AssertionError("No inline input");
    journal.addProperty("selectedText",input.getSelectedText());
    input.setText("committed");key(input,javafx.scene.input.KeyCode.ENTER);
    journal.addProperty("enterValue",table.getItems().get(1).get(3));journal.addProperty("enterClosesEditor",table.getEditingCell()==null);
    table.edit(1,table.getColumns().get(4));view.applyCss();view.layout();input=(TextField)target.lookup(".text-field");input.setText("cancelled");key(input,javafx.scene.input.KeyCode.ESCAPE);
    journal.addProperty("escapeClosesEditor",table.getEditingCell()==null);journal.addProperty("escapeValue",table.getItems().get(1).get(3));
    table.edit(1,table.getColumns().get(4));view.applyCss();view.layout();input=(TextField)target.lookup(".text-field");input.setText("blurred");
    TableCell other=null;for(var node:table.lookupAll(".table-cell"))if(node instanceof TableCell cell && cell.getIndex()==2 && cell.getTableColumn()==table.getColumns().get(4))other=cell;
    click(other,1);journal.addProperty("blurValue",table.getItems().get(1).get(3));
    table.getFocusModel().focus(1,table.getColumns().get(4));key(table,javafx.scene.input.KeyCode.F2);journal.addProperty("f2Edits",table.getEditingCell()!=null);table.edit(-1,null);
    table.getItems().get(1).set(3,"第 1 行");table.getSelectionModel().clearAndSelect(1,table.getColumns().get(4));table.edit(1,table.getColumns().get(4));
    journal.addProperty("networkRequested",false);Files.writeString(Path.of(getParameters().getRaw().get(0)+".events.json"),new GsonBuilder().setPrettyPrinting().create().toJson(journal));
    view.applyCss();view.layout();var pause=new PauseTransition(Duration.millis(250));pause.setOnFinished(done->{try{
      var path=Path.of(getParameters().getRaw().get(0));ImageIO.write(SwingFXUtils.fromFXImage(view.snapshot(null,null),null),"png",path.toFile());
      var output=new JsonObject();var texts=new JsonArray();walk(view,texts);output.add("nodes",texts);output.addProperty("networkRequested",false);
      Files.writeString(Path.of(path+".layout.json"),new GsonBuilder().setPrettyPrinting().create().toJson(output));stage.close();Platform.exit();System.exit(0);
    }catch(Throwable error){error.printStackTrace();System.exit(1);}});pause.play();
   }catch(Throwable error){error.printStackTrace();System.exit(1);}});wait.play();
  }
 }
 static void click(Node cell,int count){
 for(var type:List.of(javafx.scene.input.MouseEvent.MOUSE_PRESSED,javafx.scene.input.MouseEvent.MOUSE_RELEASED))javafx.event.Event.fireEvent(cell,new javafx.scene.input.MouseEvent(type,10,10,10,10,javafx.scene.input.MouseButton.PRIMARY,count,false,false,false,false,type==javafx.scene.input.MouseEvent.MOUSE_PRESSED,false,false,false,false,true,new javafx.scene.input.PickResult(cell,new javafx.geometry.Point3D(10,10,0),1)));
 }
 static void key(Node node,javafx.scene.input.KeyCode code){javafx.event.Event.fireEvent(node,new javafx.scene.input.KeyEvent(javafx.scene.input.KeyEvent.KEY_PRESSED,"","",code,false,false,false,false));}
 static void walk(Node node,JsonArray nodes){if(node instanceof Labeled label){var b=node.localToScene(node.getBoundsInLocal());var item=new JsonObject();item.addProperty("text",label.getText());item.addProperty("x",b.getMinX());item.addProperty("y",b.getMinY());item.addProperty("width",b.getWidth());item.addProperty("height",b.getHeight());nodes.add(item);}if(node instanceof Parent parent)for(var child:parent.getChildrenUnmodifiable())walk(child,nodes);}
}
