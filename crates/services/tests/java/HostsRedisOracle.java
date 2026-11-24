import javafx.application.*;
import javafx.animation.*;
import javafx.scene.*;
import javafx.scene.control.*;
import javafx.stage.*;
import javafx.util.Duration;
import javafx.embed.swing.SwingFXUtils;
import javafx.event.Event;
import javax.imageio.ImageIO;
import java.nio.file.*;
import java.util.*;
import com.google.gson.*;
import com.tangluobo.tomato.module.tools.HostsFilePane;
import com.tangluobo.tomato.module.connect.*;
import com.tangluobo.tomato.module.connect.view.RedisDataView;

/** Original reachable components only. No clipboard, input injection or saved endpoints. */
public class HostsRedisOracle {
 public static void main(String[] args) { Application.launch(Probe.class,args); }
 static Object field(Object value,String name)throws Exception {
  var f=value.getClass().getDeclaredField(name);f.setAccessible(true);return f.get(value);
 }
 public static class Probe extends Application {
  public void start(Stage stage)throws Exception {
   var args=getParameters().getRaw();String mode=args.get(0);Path out=Path.of(args.get(1));
   if(!System.getProperty("user.home").contains("tomato-hosts-redis-oracle-"))throw new IllegalStateException("Owned home required");
   Parent pane;
   if(mode.equals("hosts")) {
    var hosts=new HostsFilePane();
    var groups=(List<HostsFilePane.HostsGroup>)field(hosts,"hostsGroups");
    if(groups.size()!=1||!groups.get(0).getName().equals("开发验收")||groups.get(0).getEntries().size()!=2)
     throw new IllegalStateException("Original parser did not read the Rust saved configuration");
    var select=HostsFilePane.class.getDeclaredMethod("selectGroup",HostsFilePane.HostsGroup.class);select.setAccessible(true);select.invoke(hosts,groups.get(0));
    pane=hosts;
   } else {
    int port=Integer.parseInt(args.get(2));
    if(port<1||port>65535)throw new IllegalArgumentException("Loopback port required");
    var connection=new ConnectionConfig();connection.setId("owned-redis-oracle");connection.setType(ConnectType.REDIS);
    connection.setHost("127.0.0.1");connection.setPort(port);connection.setPassword("");
    pane=new RedisDataView(connection,3);
   }
   stage.initStyle(StageStyle.UNDECORATED);stage.setScene(new Scene(pane,895,600));stage.setOpacity(.01);stage.show();stage.toBack();
   var delay=new PauseTransition(Duration.millis(mode.equals("hosts")?650:1400));
   delay.setOnFinished(e->{try {
    var report=new JsonObject();report.addProperty("mode",mode);report.addProperty("ownedSceneOnly",true);
    if(mode.equals("hosts")) {
     var groups=(List<HostsFilePane.HostsGroup>)field(pane,"hostsGroups");
     report.add("groups",new Gson().toJsonTree(groups));
     var parse=HostsFilePane.class.getDeclaredMethod("parseGroups",String.class);parse.setAccessible(true);
     var source=Files.readString(Path.of(System.getProperty("user.home"),".tomata","hosts_groups.json"));
     String pretty=new GsonBuilder().setPrettyPrinting().create().toJson(JsonParser.parseString(source));
     report.addProperty("prettyConfigurationRejectedByOriginal",((List<?>)parse.invoke(pane,pretty)).isEmpty());
    } else {
     var tree=(TreeView<String>)field(pane,"keyTreeView");
     var items=tree.getRoot().getChildren();
     if(items.size()!=3)throw new IllegalStateException("Original Redis tree did not load three owned keys");
     tree.getSelectionModel().selectIndices(0,1);
     var context=tree.getContextMenu();context.getOnShowing().handle(new WindowEvent(context,WindowEvent.WINDOW_SHOWING));
     var menu=new JsonArray();for(var item:context.getItems()) {
      var m=new JsonObject();m.addProperty("text",item.getText());m.addProperty("disabled",item.isDisable());menu.add(m);
     }
     report.add("multiSelectionMenu",menu);report.addProperty("rowHeight",tree.getFixedCellSize());
    }
    pane.applyCss();pane.layout();ImageIO.write(SwingFXUtils.fromFXImage(stage.getScene().snapshot(null),null),"png",out.resolve(mode+".png").toFile());
    var nodes=new JsonArray();ToolPaneOracle.nodes(pane,nodes);report.add("nodes",nodes);
    report.addProperty("systemClipboardWritten",false);report.addProperty("physicalOsInputUsed",false);
    Files.writeString(out.resolve(mode+".json"),new GsonBuilder().setPrettyPrinting().create().toJson(report));
    stage.hide();Platform.exit();System.exit(0);
   }catch(Exception error){error.printStackTrace();System.exit(1);}});delay.play();
  }
 }
}
