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
public class TableDataWindowOracle {
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
    view.applyCss();view.layout();var pause=new PauseTransition(Duration.millis(250));pause.setOnFinished(done->{try{
      var path=Path.of(getParameters().getRaw().get(0));ImageIO.write(SwingFXUtils.fromFXImage(view.snapshot(null,null),null),"png",path.toFile());
      var output=new JsonObject();var texts=new JsonArray();walk(view,texts);output.add("nodes",texts);output.addProperty("networkRequested",false);
      Files.writeString(Path.of(path+".layout.json"),new GsonBuilder().setPrettyPrinting().create().toJson(output));stage.close();Platform.exit();System.exit(0);
    }catch(Throwable error){error.printStackTrace();System.exit(1);}});pause.play();
   }catch(Throwable error){error.printStackTrace();System.exit(1);}});wait.play();
  }
 }
 static void walk(Node node,JsonArray nodes){if(node instanceof Labeled label){var b=node.localToScene(node.getBoundsInLocal());var item=new JsonObject();item.addProperty("text",label.getText());item.addProperty("x",b.getMinX());item.addProperty("y",b.getMinY());item.addProperty("width",b.getWidth());item.addProperty("height",b.getHeight());nodes.add(item);}if(node instanceof Parent parent)for(var child:parent.getChildrenUnmodifiable())walk(child,nodes);}
}
