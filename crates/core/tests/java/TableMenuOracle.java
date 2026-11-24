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
public class TableMenuOracle {
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
    var setup=TableDataView.class.getDeclaredMethod("setupRowContextMenu");setup.setAccessible(true);setup.invoke(view);
    view.applyCss();view.layout();
    String mode=getParameters().getRaw().get(1);
    var selectorKey=get(view,"ROW_SELECTOR_COL");
    var frozen=(Parent)get(view,"frozenTablePane");
    TableCell single=null,multi=null,secondary=null;
    var table=(TableView)get(view,"tableView");
    for(var node:frozen.lookupAll(".table-cell"))if(node instanceof TableCell cell && cell.getTableColumn()!=null){
      boolean selector=Objects.equals(cell.getTableColumn().getUserData(),selectorKey) && cell.getTableView()!=table;
      if(selector && cell.getIndex()==0 && !mode.equals("cell-menu"))single=cell;
      if(selector && cell.getIndex()==2)multi=cell;
      if(selector && cell.getIndex()==1)secondary=cell;
      if(mode.equals("cell-menu") && cell.getTableColumn()==table.getColumns().get(2) && cell.getIndex()==0)single=cell;
    }
    if(single==null)throw new AssertionError("Missing original cell/selector");
    press(single,javafx.scene.input.MouseButton.PRIMARY,false);
    if(mode.equals("multi-row-menu")){if(multi==null || secondary==null)throw new AssertionError("Missing original multi-row selectors");press(multi,javafx.scene.input.MouseButton.PRIMARY,true);single=secondary;}
    press(single,javafx.scene.input.MouseButton.SECONDARY,false);
    var screen=single.localToScreen(8,14);
    javafx.event.Event.fireEvent(single,new javafx.scene.input.ContextMenuEvent(javafx.scene.input.ContextMenuEvent.CONTEXT_MENU_REQUESTED,8,14,screen.getX(),screen.getY(),false,new javafx.scene.input.PickResult(single,new javafx.geometry.Point3D(8,14,0),1)));
    var pause=new PauseTransition(Duration.millis(350));pause.setOnFinished(done->{try{
      var path=Path.of(getParameters().getRaw().get(0));
      ContextMenu menu=null;for(var window:javafx.stage.Window.getWindows())if(window instanceof ContextMenu context && context.isShowing())menu=context;
      if(menu==null)throw new AssertionError("Original context menu did not open");
      var root=menu.getScene().getRoot();root.applyCss();root.layout();
      ImageIO.write(SwingFXUtils.fromFXImage(root.snapshot(null,null),null),"png",path.toFile());
      var output=new JsonObject();var texts=new JsonArray();walk(root,texts);output.add("nodes",texts);output.addProperty("networkRequested",false);
      output.addProperty("width",root.getBoundsInLocal().getWidth());output.addProperty("height",root.getBoundsInLocal().getHeight());
      output.addProperty("mode",mode);
      output.addProperty("layoutWidth",root.getLayoutBounds().getWidth());output.addProperty("layoutHeight",root.getLayoutBounds().getHeight());
      var cellPositions=new JsonArray();for(var selected:table.getSelectionModel().getSelectedCells()){var position=(TablePosition)selected;var obj=new JsonObject();obj.addProperty("row",position.getRow());obj.addProperty("column",position.getColumn());obj.addProperty("field",position.getTableColumn()==null?null:position.getTableColumn().getText());cellPositions.add(obj);}output.add("selectedCells",cellPositions);output.add("selectedRows",new Gson().toJsonTree(table.getSelectionModel().getSelectedIndices()));
      var entries=new JsonArray();for(var item:menu.getItems()){var entry=new JsonObject();entry.addProperty("text",item.getText());entry.addProperty("separator",item instanceof SeparatorMenuItem);entry.addProperty("disabled",item.isDisable());entries.add(entry);}output.add("items",entries);
      Files.writeString(Path.of(path+".layout.json"),new GsonBuilder().setPrettyPrinting().create().toJson(output));
      menu.hide();stage.close();Platform.exit();System.exit(0);
    }catch(Throwable error){error.printStackTrace();System.exit(1);}});pause.play();
   }catch(Throwable error){error.printStackTrace();System.exit(1);}});wait.play();
  }
 }
 static void press(TableCell cell,javafx.scene.input.MouseButton button,boolean shift){
   var screen=cell.localToScreen(8,14);
   for(var kind:List.of(javafx.scene.input.MouseEvent.MOUSE_PRESSED,javafx.scene.input.MouseEvent.MOUSE_RELEASED))
    javafx.event.Event.fireEvent(cell,new javafx.scene.input.MouseEvent(kind,8,14,screen.getX(),screen.getY(),button,1,shift,false,false,false,button==javafx.scene.input.MouseButton.PRIMARY && kind==javafx.scene.input.MouseEvent.MOUSE_PRESSED,false,button==javafx.scene.input.MouseButton.SECONDARY && kind==javafx.scene.input.MouseEvent.MOUSE_PRESSED,false,button==javafx.scene.input.MouseButton.SECONDARY,true,new javafx.scene.input.PickResult(cell,new javafx.geometry.Point3D(8,14,0),1)));
 }
 static void walk(Node node,JsonArray nodes){if(node instanceof Labeled label){var b=node.localToScene(node.getBoundsInLocal());var item=new JsonObject();item.addProperty("text",label.getText());item.addProperty("font",label.getFont().getName());item.addProperty("fontSize",label.getFont().getSize());item.addProperty("x",b.getMinX());item.addProperty("y",b.getMinY());item.addProperty("width",b.getWidth());item.addProperty("height",b.getHeight());nodes.add(item);}if(node instanceof Parent parent)for(var child:parent.getChildrenUnmodifiable())walk(child,nodes);}
}
