import javafx.application.*;
import javafx.animation.PauseTransition;
import javafx.scene.*;
import javafx.scene.control.*;
import javafx.stage.Stage;
import javafx.util.Duration;
import javafx.embed.swing.SwingFXUtils;
import com.google.gson.*;
import com.tangluobo.tomato.module.connect.*;
import com.tangluobo.tomato.module.connect.service.DatabaseService;
import com.tangluobo.tomato.module.connect.view.TableStructureView;
import java.nio.file.*;
import java.util.*;
import javax.imageio.ImageIO;

/** Unchanged original new-table window; synthetic JDBC and in-memory settings only.
 * Own scene snapshots, no sockets, config reads/writes, clipboard or OS input. */
public class TableDesignWindowOracle {
 static Object field(Object value,String name)throws Exception {
  var field=TableStructureView.class.getDeclaredField(name);field.setAccessible(true);return field.get(value);
 }
 public static void main(String[] args){Application.launch(Probe.class,args);}
 public static class Probe extends Application {
  TableStructureView view;Stage stage;
  @SuppressWarnings({"unchecked","rawtypes"})
  public void start(Stage own)throws Exception {
   stage=own;
   var settings=new GlobalConfig();settings.setTableFontName("System");settings.setTableFontSize(12);
   var singleton=GlobalConfig.class.getDeclaredField("instance");singleton.setAccessible(true);singleton.set(null,settings);
   var config=new ConnectionConfig();config.setType(ConnectType.MYSQL);config.setName("隔离设计窗口");config.setId("table-design-oracle-only");
   var cache=DatabaseService.class.getDeclaredField("connectionCache");cache.setAccessible(true);
   ((Map)cache.get(null)).put(config.getId(),DatabaseObjectsWindowOracle.connection());
   view=new TableStructureView(config,"sample_db",null,null);
   // The application inherits this stylesheet from ConnectModule. A standalone
   // scene must attach it explicitly so toolbar-button styles apply as well.
   if(getParameters().getRaw().size()>2)view.getStylesheets().add(Path.of(getParameters().getRaw().get(2),"css/connect-tree.css").toUri().toString());
   stage.setScene(new Scene(view,896,650));stage.setOpacity(.01);stage.show();
   var pause=new PauseTransition(Duration.seconds(1));pause.setOnFinished(event->{try {
    var table=(TableView<?>)field(view,"tableView");
    if(table.isDisabled()||table.getItems().size()!=1)throw new AssertionError("original new-table initialization did not finish");
    if(getParameters().getRaw().size()>1){
     var titles=(List<String>)field(view,"columnTitles");
     var row=(javafx.collections.ObservableList<String>)table.getItems().get(0);
     row.set(titles.indexOf("字段名"),"id");row.set(titles.indexOf("类型"),"BIGINT");
     row.set(titles.indexOf("非空"),"是");row.set(titles.indexOf("主键"),"是");row.set(titles.indexOf("自增"),"是");
     if(getParameters().getRaw().size()>3){
      for(String[] values:new String[][]{{"name","VARCHAR","255"},{"created_at","TIMESTAMP",""}}){
       var extra=javafx.collections.FXCollections.<String>observableArrayList(Collections.nCopies(row.size(),""));
       extra.set(titles.indexOf("字段名"),values[0]);extra.set(titles.indexOf("类型"),values[1]);extra.set(titles.indexOf("长度"),values[2]);
       ((TableView)table).getItems().add(extra);
      }
     }
     table.refresh();
     ((TableView)table).getSelectionModel().select(0,(TableColumn)table.getColumns().get(1));
    }
    capture(0);
   }catch(Throwable error){fail(error);}});pause.play();
  }
  void capture(int tab)throws Exception {
   var pane=(TabPane)view.getCenter();pane.getSelectionModel().select(tab);
   var pause=new PauseTransition(Duration.millis(350));pause.setOnFinished(event->{try {
    view.applyCss();view.layout();
    var path=Path.of(getParameters().getRaw().get(0)+"-"+tab+".png");
    ImageIO.write(SwingFXUtils.fromFXImage(view.snapshot(null,null),null),"png",path.toFile());
    var result=new JsonObject();result.addProperty("width",896);result.addProperty("height",650);
    result.addProperty("selectedTab",tab);result.addProperty("tableFontName","System");result.addProperty("tableFontSize",12);
    result.addProperty("syntheticJdbcOnly",true);result.addProperty("socketsOpened",false);result.addProperty("configurationWritten",false);result.addProperty("systemClipboardWritten",false);
    var controls=new JsonArray();DatabaseObjectAlertsOracle.walk(view,controls);result.add("controls",controls);
    var inputs=new JsonArray();
    for(var node:view.lookupAll(".combo-box,.check-box,.text-field")){
     if(!node.isVisible())continue;
     var input=new JsonObject();input.addProperty("class",node.getClass().getSimpleName());input.addProperty("disabled",node.isDisabled());
     var bounds=node.localToScene(node.getLayoutBounds());input.addProperty("x",bounds.getMinX());input.addProperty("y",bounds.getMinY());input.addProperty("width",bounds.getWidth());input.addProperty("height",bounds.getHeight());
     if(node instanceof ComboBox<?> box){input.add("items",new Gson().toJsonTree(box.getItems()));input.add("value",new Gson().toJsonTree(box.getValue()));input.addProperty("editable",box.isEditable());}
     if(node instanceof CheckBox box){input.addProperty("text",box.getText());input.addProperty("selected",box.isSelected());}
     if(node instanceof TextField text){input.addProperty("value",text.getText());input.addProperty("editable",text.isEditable());}
     inputs.add(input);
    }
    result.add("inputs",inputs);result.add("dataTypes",new Gson().toJsonTree(field(view,"cachedDataTypes")));
    var table=(TableView<?>)field(view,"tableView");var columns=new JsonArray();
    for(var column:table.getColumns()){
     var row=new JsonObject();row.addProperty("title",column.getText());row.addProperty("width",column.getWidth());row.addProperty("visible",column.isVisible());columns.add(row);
    }
    result.add("columns",columns);result.addProperty("fixedCellSize",table.getFixedCellSize());
    Files.writeString(Path.of(path+".json"),new GsonBuilder().setPrettyPrinting().create().toJson(result));
    if(tab<6)capture(tab+1);else{stage.close();Platform.exit();System.exit(0);}
   }catch(Throwable error){fail(error);}});pause.play();
  }
  void fail(Throwable error){error.printStackTrace();System.exit(1);}
 }
}
